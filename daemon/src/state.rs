use crate::async_pty::{AsyncPty, PtyReadResult};
use common::{
    ctl::{ConnectionInfo, CtlMessage, CtlResponse},
    msg::{encode_message, MessageHeader, MAX_MESSAGE_LEN},
};
use nix::{fcntl::OFlag, pty};
use std::net::Ipv4Addr;
use tokio::{io::AsyncWriteExt, net::TcpStream, select, sync::oneshot};
use tokio_util::bytes::BytesMut;

#[allow(unused)]
pub struct Connection {
    name: String,
    addr: u32,
    port: u16,
    slave_path: String,

    shutdown_tx: oneshot::Sender<()>,
}

#[derive(Default)]
pub struct State {
    conns: Vec<Connection>,
}

impl State {
    pub async fn handle_msg(&mut self, msg: CtlMessage) -> CtlResponse {
        match msg {
            CtlMessage::Add { name, addr, port } => {
                // Check if name is already used
                if self.conns.iter().any(|c| c.name == name) {
                    return CtlResponse::Error(format!(
                        "Connection with name '{}' already exists",
                        name
                    ));
                };

                let master = match pty::posix_openpt(OFlag::O_RDWR | OFlag::O_NOCTTY) {
                    Ok(master) => master,
                    Err(e) => return CtlResponse::Error(format!("Failed to create pty: {}", e)),
                };
                if let Err(e) = pty::grantpt(&master) {
                    return CtlResponse::Error(format!("Failed to grant pty: {}", e));
                }
                if let Err(e) = pty::unlockpt(&master) {
                    return CtlResponse::Error(format!("Failed to unlock pty: {}", e));
                }

                let slave_name = match unsafe { pty::ptsname(&master) } {
                    Ok(name) => name,
                    Err(e) => return CtlResponse::Error(format!("Failed to get pts name: {}", e)),
                };

                let master = match AsyncPty::new(master) {
                    Ok(master) => master,
                    Err(e) => {
                        eprintln!("Failed to create async pty: {e}");
                        return CtlResponse::Error("Something went wrong".into());
                    }
                };

                let stream = match TcpStream::connect((Ipv4Addr::from_bits(addr), port)).await {
                    Ok(stream) => stream,
                    Err(e) => {
                        return CtlResponse::Error(format!(
                            "Failed to connect to {}:{port} - {e}",
                            Ipv4Addr::from_bits(addr)
                        ));
                    }
                };

                let (shutdown_tx, shutdown_rx) = oneshot::channel();

                tokio::spawn(conn_task(master, stream, shutdown_rx));
                self.conns.push(Connection {
                    name,
                    addr,
                    port,
                    shutdown_tx,
                    slave_path: slave_name.clone(),
                });
                CtlResponse::AddOk(slave_name)
            }
            CtlMessage::Remove { name } => {
                if let Some(pos) = self.conns.iter().position(|c| c.name == name) {
                    let conn = self.conns.swap_remove(pos);
                    // If send errors, it means the task has already shut down, so we can ignore it
                    let _ = conn.shutdown_tx.send(());
                    CtlResponse::RemoveOk
                } else {
                    CtlResponse::Error(format!("No connection found with name: {name}"))
                }
            }
            CtlMessage::List => {
                let list = self
                    .conns
                    .iter()
                    .map(|c| ConnectionInfo {
                        name: c.name.clone(),
                        addr: c.addr,
                        port: c.port,
                        pts_path: c.slave_path.clone(),
                    })
                    .collect();
                CtlResponse::List(list)
            }
        }
    }
}

async fn conn_task(
    mut master: AsyncPty,
    mut sock: TcpStream,
    mut shutdown_rx: oneshot::Receiver<()>,
) {
    let mut pty_buf = [0; MAX_MESSAGE_LEN as usize];
    let mut sock_buf = BytesMut::new();
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
                        let data = String::from_utf8_lossy(&pty_buf[..n]);
                        println!("Received from pty: {data}");
                        encode_message(MessageHeader::data(n as u8), &pty_buf[..n], &mut sock_buf).expect("Something went wrong");
                        sock.write_all(&sock_buf).await.expect("Failed to send data to sock");
                    }
                    Ok(PtyReadResult::ControlMessage(c)) => {
                        println!("Received other ctrl message: {}", c);
                    }
                    Err(e) => {
                        eprintln!("Failed to read from pty: {e} {}", e.kind());
                        continue;
                    }
                }
            }
        }
    }
}
