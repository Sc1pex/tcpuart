use std::io;

use common::CtlMessage;
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
    pub fn handle_msg(&mut self, msg: CtlMessage) -> io::Result<()> {
        match msg {
            CtlMessage::Add { name, addr, port } => {
                // Create pty
                let master = pty::posix_openpt(OFlag::O_RDWR)?;
                pty::grantpt(&master)?;
                pty::unlockpt(&master)?;

                let slave_name = unsafe { pty::ptsname(&master)? };

                println!("pts path: {}", slave_name);

                self.conns.push(Connection {
                    name,
                    addr,
                    port,
                    socket: None,
                    pty_master: master,
                });
            }
            CtlMessage::Remove { name: _name } => todo!(),
            CtlMessage::List => todo!(),
        }

        Ok(())
    }
}
