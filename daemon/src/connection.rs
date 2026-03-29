use crate::async_pty::{AsyncPty, PtyReadResult};
use common::msg::{Message, MessageDecoder, MessageEncoder, MAX_MESSAGE_LEN};
use futures::{SinkExt, StreamExt};
use tokio::{io::AsyncWriteExt, net::TcpStream, select, sync::oneshot};
use tokio_util::codec::{FramedRead, FramedWrite};

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
    mut master: AsyncPty,
    mut sock: TcpStream,
    mut shutdown_rx: oneshot::Receiver<()>,
) {
    let (reader, writer) = sock.split();
    let mut pty_buf = [0; MAX_MESSAGE_LEN];
    let mut writer = FramedWrite::new(writer, MessageEncoder);
    let mut reader = FramedRead::new(reader, MessageDecoder);
    loop {
        select! {
            _ = &mut shutdown_rx => {
                println!("Shutting down connection task");
                break;
            }
            res = master.read(&mut pty_buf) => {
                match res {
                    Ok(PtyReadResult::TermiosChange(c)) => {
                        println!("Termios settings changed: ");
                        println!("   Baud: {}", c.baudrate);
                        println!("   Data bits: {}", c.data_bits);
                        println!("   Parity: {}", c.parity);
                        println!("   Stop bits: {}", c.stop_bits);
                    }
                    Ok(PtyReadResult::Data(n)) => {
                        if writer.send(pty_buf[..n].into()).await.is_err() {
                            // TODO: kill the connection or mark the socket
                            // as disconnected and try to reconnect later
                            break;
                        }
                    }
                    Ok(PtyReadResult::ControlMessage(c)) => {
                        println!("Received other ctrl message: {c}");
                    }
                    Err(e) => {
                        eprintln!("Failed to read from pty: {e}");
                    }
                }
            }
            Some(msg) = reader.next() => {
                match msg {
                    Ok(msg) => {
                        if let Message::Data(size, data) = msg {
                            let _ = master.write_all(&data[..size as usize]).await;
                        }
                    }
                    Err(e) => {
                        eprintln!("Received invalid message from server: {e}");
                    }
                }
            }
        }
    }
}
