use crate::{
    async_pty::{AsyncPty, PtyReadResult},
    event::DaemonEvent,
    tcp_bridge::TcpBridge,
};
use common::msg::{Message, MAX_MESSAGE_LEN};
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

    pub shutdown_tx: oneshot::Sender<()>,
}

impl Connection {
    pub fn new(
        name: String,
        addr: u32,
        port: u16,
        slave_path: String,
    ) -> (Self, oneshot::Receiver<()>) {
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        (
            Connection {
                name,
                addr,
                port,
                slave_path,
                shutdown_tx,
            },
            shutdown_rx,
        )
    }
}

pub async fn conn_task(
    conn_name: String,
    mut master: AsyncPty,
    mut shutdown_rx: oneshot::Receiver<()>,
    mut tcp: TcpBridge,
    event_tx: mpsc::Sender<DaemonEvent>,
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
                        } else {
                            eprintln!("Received unexpected message: {msg:?}");
                        }
                    }
                    Err(_) => {
                        break;
                    }
                }
            }
        }
    }

    let _ = event_tx
        .send(DaemonEvent::ConnectionClosed(conn_name))
        .await;
}
