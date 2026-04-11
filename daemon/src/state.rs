use crate::connection::Connection;
use common::{
    ctl::ConnectionInfo,
    msg::{MessageControlReq, MessageControlRes},
};

#[derive(Default)]
pub struct State {
    conns: Vec<Connection>,
}

impl State {
    pub fn add(&mut self, conn: Connection) {
        self.conns.push(conn);
    }

    pub fn remove(&mut self, name: &str) -> bool {
        let Some(pos) = self.conns.iter().position(|c| c.name == name) else {
            return false;
        };

        let conn = self.conns.swap_remove(pos);
        conn.shutdown();
        true
    }

    pub fn list(&self) -> Vec<ConnectionInfo> {
        self.conns
            .iter()
            .map(|c| ConnectionInfo {
                name: c.name.clone(),
                addr: c.addr,
                port: c.port,
                pts_path: c.slave_path.clone(),
            })
            .collect()
    }

    pub async fn send_ctl_msg(
        &mut self,
        conn_name: &str,
        ctl: MessageControlReq,
    ) -> Option<MessageControlRes> {
        let conn = self.conns.iter_mut().find(|c| c.name == conn_name)?;
        conn.send_ctl_msg(ctl).await
    }

    pub fn exists(&self, name: &str) -> bool {
        self.conns.iter().any(|c| c.name == name)
    }
}
