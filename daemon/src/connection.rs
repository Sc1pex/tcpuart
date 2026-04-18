use crate::{
    async_pty::{AsyncPty, PtyReadResult},
    event::DaemonEvent,
    tcp_bridge::{TcpBridge, TcpBridgeStatus},
};
use common::msg::{MAX_MESSAGE_LEN, Message, MessageControlReq, MessageControlRes};
use std::time::Duration;
use tokio::{
    io::AsyncWriteExt,
    select,
    sync::{mpsc, oneshot},
};
use tracing::{error, info, instrument};

pub struct Connection {
    pub name: String,
    pub addr: u32,
    pub port: u16,
    pub slave_path: String,

    shutdown_tx: oneshot::Sender<()>,
    ctl_req_tx: mpsc::Sender<MessageControlReq>,
    ctl_res_rx: mpsc::Receiver<MessageControlRes>,
}

impl Connection {
    pub fn build_and_spawn(
        name: String,
        addr: u32,
        port: u16,
        master: AsyncPty,
        slave_path: String,
        event_tx: mpsc::Sender<DaemonEvent>,
    ) -> Self {
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let tcp = TcpBridge::new(addr, port);
        let (ctl_req_tx, ctl_req_rx) = mpsc::channel(8);
        let (ctl_res_tx, ctl_res_rx) = mpsc::channel(8);

        let conn = Self {
            name: name.clone(),
            addr,
            port,
            slave_path,
            shutdown_tx,
            ctl_req_tx,
            ctl_res_rx,
        };

        let task = ConnectionTask {
            conn_name: name,
            master,
            shutdown_rx,
            event_tx,
            tcp,
            ctl_req_rx,
            ctl_res_tx,
            pty_buf: [0; MAX_MESSAGE_LEN],
            state: ConnState::Reconnecting {
                backoff: INITIAL_RECONNECT_BACKOFF,
                pending: None,
            },
        };
        tokio::spawn(conn_task(task));

        conn
    }

    pub async fn send_ctl_msg(&mut self, ctl: MessageControlReq) -> Option<MessageControlRes> {
        if self.ctl_req_tx.send(ctl).await.is_err() {
            return None;
        }
        self.ctl_res_rx.recv().await
    }

    pub fn shutdown(self) {
        // If send errors, it means the task has already shut down, so we can ignore it
        let _ = self.shutdown_tx.send(());
    }
}

enum ConnState {
    Connected,
    Reconnecting {
        backoff: Duration,
        pending: Option<Message>,
    },
    Resyncing {
        pending: Message,
    },
    ShuttingDown,
}

struct ConnectionTask {
    conn_name: String,
    master: AsyncPty,
    tcp: TcpBridge,
    shutdown_rx: oneshot::Receiver<()>,
    event_tx: mpsc::Sender<DaemonEvent>,
    ctl_req_rx: mpsc::Receiver<MessageControlReq>,
    ctl_res_tx: mpsc::Sender<MessageControlRes>,
    state: ConnState,
    pty_buf: [u8; MAX_MESSAGE_LEN],
}

#[allow(dead_code)]
impl ConnectionTask {
    async fn step(&mut self) {
        let next_state = match self.state {
            ConnState::Connected => self.step_connected().await,
            ConnState::Reconnecting { .. } => self.step_reconnecting().await,
            ConnState::Resyncing { .. } => self.step_resyncing().await,
            ConnState::ShuttingDown => None,
        };

        if let Some(state) = next_state {
            self.state = state;
        }
    }

    async fn step_connected(&mut self) -> Option<ConnState> {
        select! {
            _ = &mut self.shutdown_rx => {
                info!("shutting down connection task");
                Some(ConnState::ShuttingDown)
            }
            res = self.master.read(&mut self.pty_buf) => {
                match res {
                    Ok(PtyReadResult::TermiosChange(c)) => {
                        let msg = Message::Config {
                            baudrate: c.baudrate,
                            data_bits: c.data_bits,
                            stop_bits: c.stop_bits,
                            parity: c.parity
                        };
                        match self.tcp.send(msg).await {
                            TcpBridgeStatus::Ok(()) => None,
                            TcpBridgeStatus::Disconnected(e) => {
                                let backoff = INITIAL_RECONNECT_BACKOFF;
                                error!(error = %e, "connection lost, retrying in {:?}", backoff);
                                Some(ConnState::Reconnecting {
                                    backoff,
                                    pending: Some(msg),
                                })
                            }
                        }
                    }
                    Ok(PtyReadResult::Data(n)) => {
                        let msg: Message = self.pty_buf[..n].into();
                        match self.tcp.send(msg).await {
                            TcpBridgeStatus::Ok(()) => None,
                            TcpBridgeStatus::Disconnected(e) => {
                                let backoff = INITIAL_RECONNECT_BACKOFF;
                                error!(error = %e, "connection lost, retrying in {:?}", backoff);
                                Some(ConnState::Reconnecting {
                                    backoff,
                                    pending: Some(msg),
                                })
                            }
                        }
                    }
                    Ok(PtyReadResult::ControlMessage(c)) => {
                        error!(control_message = ?c, "unexpected control message from pty");
                        None
                    }
                    Err(e) => {
                        error!(error = %e, "error reading from pty");
                        Some(ConnState::ShuttingDown)
                    }
                }
            }
            msg = self.tcp.next() => {
                match msg {
                    TcpBridgeStatus::Ok(msg) => {
                        match msg {
                            Message::Data(size, data) => {
                                if let Err(e) = self.master.write_all(&data[..size as usize]).await {
                                    error!(error = %e, "failed to write data to pty");
                                    return Some(ConnState::ShuttingDown);
                                }
                            }
                            Message::ControlRes(resp) => {
                                if self.ctl_res_tx.send(resp).await.is_err() {
                                    return Some(ConnState::ShuttingDown);
                                }
                            }
                            _ => error!(?msg, "unexpected message from tcp bridge"),
                        }
                        None
                    }
                    TcpBridgeStatus::Disconnected(e) => {
                        let backoff = INITIAL_RECONNECT_BACKOFF;
                        error!(error = %e, "connection lost, retrying in {:?}", backoff);
                        Some(ConnState::Reconnecting {
                            backoff,
                            pending: None,
                        })
                    },
                }
            }
            ctl_msg = self.ctl_req_rx.recv() => {
                let Some(ctl_msg) = ctl_msg else {
                    return Some(ConnState::ShuttingDown);
                };

                let msg = Message::ControlReq(ctl_msg);
                match self.tcp.send(msg).await {
                    TcpBridgeStatus::Ok(()) => None,
                    TcpBridgeStatus::Disconnected(e) => {
                        let backoff = INITIAL_RECONNECT_BACKOFF;
                        error!(error = %e, "connection lost, retrying in {:?}", backoff);
                        if self.ctl_res_tx.send(MessageControlRes::NotSupported).await.is_err() {
                            Some(ConnState::ShuttingDown)
                        } else {
                            Some(ConnState::Reconnecting {
                                backoff,
                                pending: None,
                            })
                        }
                    }
                }
            }
        }
    }

    async fn step_reconnecting(&mut self) -> Option<ConnState> {
        let (backoff, pending) = match self.state {
            ConnState::Reconnecting { backoff, pending } => (backoff, pending),
            _ => unreachable!(),
        };

        let reconnect_sleep = tokio::time::sleep(backoff);
        tokio::pin!(reconnect_sleep);

        loop {
            select! {
                _ = &mut self.shutdown_rx => {
                    info!("shutting down connection task");
                    return Some(ConnState::ShuttingDown)
                }
                ctl_msg = self.ctl_req_rx.recv() => {
                    let Some(_) = ctl_msg else {
                        return Some(ConnState::ShuttingDown);
                    };

                    if self.ctl_res_tx.send(MessageControlRes::NotSupported).await.is_err() {
                        return Some(ConnState::ShuttingDown);
                    }
                }
                _ = &mut reconnect_sleep => break,
            }
        }

        match self.tcp.try_connect().await {
            TcpBridgeStatus::Ok(()) => {
                if let Some(pending) = pending {
                    Some(ConnState::Resyncing { pending })
                } else {
                    Some(ConnState::Connected)
                }
            }
            TcpBridgeStatus::Disconnected(e) => {
                let next_backoff = Self::next_backoff(backoff);
                error!(error = %e, "reconnect failed, retrying in {:?}", next_backoff);
                Some(ConnState::Reconnecting {
                    backoff: next_backoff,
                    pending,
                })
            }
        }
    }

    async fn step_resyncing(&mut self) -> Option<ConnState> {
        let pending = match self.state {
            ConnState::Resyncing { pending } => pending,
            _ => unreachable!(),
        };

        let send_pending = self.tcp.send(pending);
        tokio::pin!(send_pending);

        loop {
            select! {
                _ = &mut self.shutdown_rx => {
                    info!("shutting down connection task");
                    return Some(ConnState::ShuttingDown)
                }
                ctl_msg = self.ctl_req_rx.recv() => {
                    let Some(_) = ctl_msg else {
                        return Some(ConnState::ShuttingDown);
                    };

                    if self.ctl_res_tx.send(MessageControlRes::NotSupported).await.is_err() {
                        return Some(ConnState::ShuttingDown);
                    }
                }
                send_status = &mut send_pending => {
                    return match send_status {
                        TcpBridgeStatus::Ok(()) => Some(ConnState::Connected),
                        TcpBridgeStatus::Disconnected(e) => {
                            let backoff = INITIAL_RECONNECT_BACKOFF;
                            error!(error = %e, "resync failed, retrying in {:?}", backoff);
                            Some(ConnState::Reconnecting {
                                backoff,
                                pending: Some(pending),
                            })
                        }
                    };
                }
            }
        }
    }

    async fn shutdown(&mut self) {
        info!("connection task exiting, notifying daemon");
        let _ = self
            .event_tx
            .send(DaemonEvent::ConnectionClosed(self.conn_name.clone()))
            .await;
    }

    fn should_close(&self) -> bool {
        matches!(self.state, ConnState::ShuttingDown)
    }

    fn next_backoff(backoff: Duration) -> Duration {
        (backoff * 2).min(MAX_RECONNECT_BACKOFF)
    }
}

const INITIAL_RECONNECT_BACKOFF: Duration = Duration::from_millis(100);
const MAX_RECONNECT_BACKOFF: Duration = Duration::from_secs(5);

#[instrument(skip(task) fields(conn_name = task.conn_name, remote_addr = %task.tcp.addr(), remote_port = %task.tcp.port()))]
async fn conn_task(mut task: ConnectionTask) {
    while !task.should_close() {
        task.step().await;
    }
    task.shutdown().await;
}
