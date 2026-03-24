use crate::async_pty::{AsyncPty, PtyReadResult};
use common::{CtlMessage, CtlResponse};
use nix::{
    fcntl::{self, OFlag},
    pty,
};
use std::os::fd::OwnedFd;
use tokio::net::TcpStream;

#[allow(unused)]
pub struct Connection {
    name: String,
    addr: u32,
    port: u16,

    socket: Option<TcpStream>,
    // Keep a slave connection alive to prevent EIO errors
    keep_alive: OwnedFd,
}

#[derive(Default)]
pub struct State {
    conns: Vec<Connection>,
}

impl State {
    pub fn handle_msg(&mut self, msg: CtlMessage) -> CtlResponse {
        match msg {
            CtlMessage::Add { name, addr, port } => {
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
                    Err(_) => {
                        eprintln!("Failed to create async pty");
                        return CtlResponse::Error("Something went wrong".into());
                    }
                };

                let keep_alive = match fcntl::open(
                    slave_name.as_str(),
                    OFlag::O_RDWR | OFlag::O_NOCTTY,
                    nix::sys::stat::Mode::empty(),
                ) {
                    Ok(fd) => fd,
                    Err(e) => {
                        eprintln!("Failed to create owned fd: {e}");
                        return CtlResponse::Error("Something went wrong".into());
                    }
                };

                tokio::spawn(conn_task(master));
                self.conns.push(Connection {
                    name,
                    addr,
                    port,
                    socket: None,
                    keep_alive,
                });
                CtlResponse::AddOk(slave_name)
            }
            CtlMessage::Remove { name: _name } => todo!(),
            CtlMessage::List => todo!(),
        }
    }
}

async fn conn_task(mut master: AsyncPty) {
    let mut buf = [0; 128];
    loop {
        match master.read(&mut buf).await {
            Ok(PtyReadResult::TermiosChange) => {
                println!("Termios settings changed");
            }
            Ok(PtyReadResult::Data(n)) => {
                let data = String::from_utf8_lossy(&buf[..n]);
                println!("Received from pty: {data}");
            }
            Ok(PtyReadResult::ControlMessage(c)) => {
                println!("Received other ctrl message: {}", c);
            }
            Err(e) => {
                eprintln!("Failed to read from pty: {e} {}", e.kind());
                continue;
            }
        };
    }
}
