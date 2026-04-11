use crate::{
    async_pty::{AsyncPty, PtyReadResult},
    event::DaemonEvent,
    tcp_bridge::TcpBridge,
};
use common::msg::{MAX_MESSAGE_LEN, Message, MessageControlReq, MessageControlRes};
use tokio::{
    io::AsyncWriteExt,
    select,
    sync::{mpsc, oneshot},
};

pub struct Connection {
    pub name: String,
    pub addr: u32,
    pub port: u16,
    pub slave_path: String,

    shutdown_tx: oneshot::Sender<()>,
    ctl_req_tx: mpsc::Sender<MessageControlReq>,
    ctl_res_rx: mpsc::Receiver<MessageControlRes>,
}

struct ConnectionTaskParams {
    conn_name: String,
    master: AsyncPty,
    tcp: TcpBridge,
    shutdown_rx: oneshot::Receiver<()>,
    event_tx: mpsc::Sender<DaemonEvent>,
    ctl_req_rx: mpsc::Receiver<MessageControlReq>,
    ctl_res_tx: mpsc::Sender<MessageControlRes>,
}

pub struct ConnectionBuilder {
    conn: Connection,
    task_params: Option<ConnectionTaskParams>,
}

impl ConnectionBuilder {
    pub fn new(
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

        Self {
            conn: Connection {
                name: name.clone(),
                addr,
                port,
                slave_path,
                shutdown_tx,
                ctl_req_tx,
                ctl_res_rx,
            },
            task_params: Some(ConnectionTaskParams {
                conn_name: name,
                master,
                shutdown_rx,
                event_tx,
                tcp,
                ctl_req_rx,
                ctl_res_tx,
            }),
        }
    }

    pub fn build_and_spawn(mut self) -> Connection {
        let task_params = self
            .task_params
            .take()
            .expect("Task params should be present");
        tokio::spawn(conn_task(task_params));
        self.conn
    }
}

impl Connection {
    pub fn shutdown(self) {
        // If send errors, it means the task has already shut down, so we can ignore it
        let _ = self.shutdown_tx.send(());
    }

    pub async fn send_ctl_msg(&mut self, ctl: MessageControlReq) -> Option<MessageControlRes> {
        if self.ctl_req_tx.send(ctl).await.is_err() {
            return None;
        }
        self.ctl_res_rx.recv().await
    }
}

async fn conn_task(
    ConnectionTaskParams {
        conn_name,
        mut master,
        mut shutdown_rx,
        mut tcp,
        event_tx,
        mut ctl_req_rx,
        ctl_res_tx,
    }: ConnectionTaskParams,
) {
    let mut pty_buf = [0; MAX_MESSAGE_LEN];

    loop {
        select! {
            _ = &mut shutdown_rx => {
                println!("Shutting down connection task");
                break;
            }
            res = master.read(&mut pty_buf) => {
                match res {
                    Ok(PtyReadResult::TermiosChange(c)) => {
                        let msg = Message::Config {
                            baudrate: c.baudrate,
                            data_bits: c.data_bits,
                            stop_bits: c.stop_bits,
                            parity: c.parity
                        };
                        if tcp.send(msg).await.is_err() {
                            break;
                        }
                    }
                    Ok(PtyReadResult::Data(n)) => {
                        if tcp.send(pty_buf[..n].into()).await.is_err() {
                            break;
                        }
                    }
                    Ok(PtyReadResult::ControlMessage(c)) => {
                        println!("Received other ctrl message: {c}");
                    }
                    Err(e) => {
                        eprintln!("Failed to read from pty: {e}");
                        break;
                    }
                }
            }
            msg = tcp.next() => {
                match msg {
                    Ok(msg) => {
                        if let Message::Data(size, data) = msg {
                            let _ = master.write_all(&data[..size as usize]).await;
                        } else if let Message::ControlRes(resp) = msg {
                            if ctl_res_tx.send(resp).await.is_err() {
                                break;
                            }
                        } else {
                            eprintln!("Received unexpected message: {msg:?}");
                        }
                    }
                    Err(_) => {
                        break;
                    }
                }
            }
            ctl_msg = ctl_req_rx.recv() => {
                if let Some(ctl_msg) = ctl_msg {
                    let msg = Message::ControlReq(ctl_msg);
                    if tcp.send(msg).await.is_err() {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
    }

    let _ = event_tx
        .send(DaemonEvent::ConnectionClosed(conn_name))
        .await;
}
