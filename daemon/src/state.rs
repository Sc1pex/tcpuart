use crate::connection::Connection;
use common::ctl::ConnectionInfo;

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

    pub fn exists(&self, name: &str) -> bool {
        self.conns.iter().any(|c| c.name == name)
    }
}
