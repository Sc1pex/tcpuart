use crate::{
    async_pty::{AsyncPty, PtyReadResult},
    event::DaemonEvent,
    tcp_bridge::{TcpBridge, TcpBridgeStatus},
};
use common::msg::{DeviceControlRequest, DeviceControlResponse, DeviceMessage, MAX_MESSAGE_LEN};
use std::time::Duration;
use tokio::{
    io::AsyncWriteExt,
    select,
    sync::{mpsc, oneshot},
};
use tracing::{error, info, instrument};

#[derive(Debug, Clone, Copy)]
pub enum DeviceControlError {
    /// The device is currently unavailable (disconnected or busy).
    Unavailable,
}

/// Internal requests sent from the `Connection` object to the background `conn_task`.
pub enum ConnectionTaskRequest {
    HardwareControl(
        DeviceControlRequest,
        oneshot::Sender<Result<DeviceControlResponse, DeviceControlError>>,
    ),
    Status(oneshot::Sender<bool>),
}

pub struct Connection {
    pub name: String,
    pub addr: u32,
    pub port: u16,
    pub slave_path: String,

    shutdown_tx: oneshot::Sender<()>,
    ctl_req_tx: mpsc::Sender<ConnectionTaskRequest>,
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

        let conn = Self {
            name: name.clone(),
            addr,
            port,
            slave_path,
            shutdown_tx,
            ctl_req_tx,
        };

        let task = ConnectionTask {
            conn_name: name,
            master,
            shutdown_rx,
            event_tx,
            tcp,
            ctl_req_rx,
            pty_buf: [0; MAX_MESSAGE_LEN],
            state: ConnState::Reconnecting {
                backoff: INITIAL_RECONNECT_BACKOFF,
                pending: None,
            },
            pending_hw_ctl: None,
        };
        tokio::spawn(conn_task(task));

        conn
    }

    pub async fn send_hardware_ctl(
        &mut self,
        ctl: DeviceControlRequest,
    ) -> Option<Result<DeviceControlResponse, DeviceControlError>> {
        let (tx, rx) = oneshot::channel();
        if self
            .ctl_req_tx
            .send(ConnectionTaskRequest::HardwareControl(ctl, tx))
            .await
            .is_err()
        {
            return None;
        }
        rx.await.ok()
    }

    pub async fn get_status(&self) -> bool {
        let (tx, rx) = oneshot::channel();
        if self
            .ctl_req_tx
            .send(ConnectionTaskRequest::Status(tx))
            .await
            .is_err()
        {
            return false;
        }
        rx.await.unwrap_or(false)
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
        pending: Option<DeviceMessage>,
    },
    Resyncing {
        pending: DeviceMessage,
    },
    ShuttingDown,
}

struct ConnectionTask {
    conn_name: String,
    master: AsyncPty,
    tcp: TcpBridge,
    shutdown_rx: oneshot::Receiver<()>,
    event_tx: mpsc::Sender<DaemonEvent>,
    ctl_req_rx: mpsc::Receiver<ConnectionTaskRequest>,
    state: ConnState,
    pty_buf: [u8; MAX_MESSAGE_LEN],

    pending_hw_ctl: Option<oneshot::Sender<Result<DeviceControlResponse, DeviceControlError>>>,
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
                        let msg = DeviceMessage::Config {
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
                                self.fail_pending_ctl();
                                Some(ConnState::Reconnecting {
                                    backoff,
                                    pending: Some(msg),
                                })
                            }
                        }
                    }

                    Ok(PtyReadResult::Data(n)) => {
                        let msg: DeviceMessage = self.pty_buf[..n].into();
                        match self.tcp.send(msg).await {
                            TcpBridgeStatus::Ok(()) => None,
                            TcpBridgeStatus::Disconnected(e) => {
                                let backoff = INITIAL_RECONNECT_BACKOFF;
                                error!(error = %e, "connection lost, retrying in {:?}", backoff);
                                self.fail_pending_ctl();
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
                            DeviceMessage::Data(size, data) => {
                                if let Err(e) = self.master.write_all(&data[..size as usize]).await {
                                    error!(error = %e, "failed to write data to pty");
                                    return Some(ConnState::ShuttingDown);
                                }
                            }

                            DeviceMessage::ControlRes(resp) => {
                                if let Some(tx) = self.pending_hw_ctl.take() {
                                    let _ = tx.send(Ok(resp));
                                } else {
                                    error!(?resp, "received unsolicited control response from hardware");
                                }
                            }

                            _ => error!(?msg, "unexpected message from tcp bridge"),
                        }
                        None
                    }

                    TcpBridgeStatus::Disconnected(e) => {
                        let backoff = INITIAL_RECONNECT_BACKOFF;
                        error!(error = %e, "connection lost, retrying in {:?}", backoff);
                        self.fail_pending_ctl();
                        Some(ConnState::Reconnecting {
                            backoff,
                            pending: None,
                        })
                    },
                }
            }

            req = self.ctl_req_rx.recv() => {
                match req {
                    Some(ConnectionTaskRequest::HardwareControl(ctl, tx)) => {
                        if self.pending_hw_ctl.is_some() {
                             let _ = tx.send(Err(DeviceControlError::Unavailable));
                             return None;
                        }

                        let msg = DeviceMessage::ControlReq(ctl);
                        match self.tcp.send(msg).await {
                            TcpBridgeStatus::Ok(()) => {
                                self.pending_hw_ctl = Some(tx);
                                None
                            }
                            TcpBridgeStatus::Disconnected(e) => {
                                let backoff = INITIAL_RECONNECT_BACKOFF;
                                error!(error = %e, "connection lost, retrying in {:?}", backoff);
                                let _ = tx.send(Err(DeviceControlError::Unavailable));
                                Some(ConnState::Reconnecting {
                                    backoff,
                                    pending: None,
                                })
                            }
                        }
                    }

                    Some(ConnectionTaskRequest::Status(tx)) => {
                        let _ = tx.send(true);
                        return None;
                    }

                    None => Some(ConnState::ShuttingDown),
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

                req = self.ctl_req_rx.recv() => {
                    match req {
                        Some(ConnectionTaskRequest::HardwareControl(_, tx)) => {
                            let _ = tx.send(Err(DeviceControlError::Unavailable));
                        }
                        Some(ConnectionTaskRequest::Status(tx)) => {
                            let _ = tx.send(false);
                        }
                        None => return Some(ConnState::ShuttingDown),
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

                req = self.ctl_req_rx.recv() => {
                    match req {
                        Some(ConnectionTaskRequest::HardwareControl(_, tx)) => {
                            let _ = tx.send(Err(DeviceControlError::Unavailable));
                        }
                        Some(ConnectionTaskRequest::Status(tx)) => {
                            let _ = tx.send(false);
                        }
                        None => return Some(ConnState::ShuttingDown),
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
        self.fail_pending_ctl();
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

    fn fail_pending_ctl(&mut self) {
        if let Some(tx) = self.pending_hw_ctl.take() {
            let _ = tx.send(Err(DeviceControlError::Unavailable));
        }
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
