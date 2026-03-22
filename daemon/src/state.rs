use common::{CtlMessage, CtlResponse};
use nix::{fcntl::OFlag, pty};
use tokio::net::TcpStream;

#[allow(unused)]
pub struct Connection {
    name: String,
    addr: u32,
    port: u16,

    socket: Option<TcpStream>,
    pty_master: pty::PtyMaster,
}

#[derive(Default)]
pub struct State {
    conns: Vec<Connection>,
}

impl State {
    pub fn handle_msg(&mut self, msg: CtlMessage) -> CtlResponse {
        match msg {
            CtlMessage::Add { name, addr, port } => {
                let master = match pty::posix_openpt(OFlag::O_RDWR) {
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

                self.conns.push(Connection {
                    name,
                    addr,
                    port,
                    socket: None,
                    pty_master: master,
                });
                CtlResponse::AddOk(slave_name)
            }
            CtlMessage::Remove { name: _name } => todo!(),
            CtlMessage::List => todo!(),
        }
    }
}
