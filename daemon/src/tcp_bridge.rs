use common::msg::{Message, MessageCodec};
use futures::{SinkExt, StreamExt};
use std::{io, net::Ipv4Addr};
use tokio::net::TcpStream;
use tokio_util::codec::Framed;
use tracing::{error, info};

#[derive(Debug)]
pub enum TcpBridgeStatus<T> {
    Disconnected(io::Error),
    Ok(T),
}

pub struct TcpBridge {
    addr: u32,
    port: u16,
    framed: Option<Framed<TcpStream, MessageCodec>>,
}

impl TcpBridge {
    pub fn new(addr: u32, port: u16) -> Self {
        Self {
            addr,
            port,
            framed: None,
        }
    }

    pub fn addr(&self) -> Ipv4Addr {
        Ipv4Addr::from_bits(self.addr)
    }
    pub fn port(&self) -> u16 {
        self.port
    }

    pub async fn send(&mut self, msg: Message) -> TcpBridgeStatus<()> {
        let Some(framed) = self.framed.as_mut() else {
            return TcpBridgeStatus::Disconnected(io::Error::new(
                io::ErrorKind::NotConnected,
                "not connected",
            ));
        };

        match framed.send(msg).await {
            Ok(_) => TcpBridgeStatus::Ok(()),
            Err(e) => {
                self.framed = None;
                TcpBridgeStatus::Disconnected(e)
            }
        }
    }

    pub async fn next(&mut self) -> TcpBridgeStatus<Message> {
        let Some(framed) = self.framed.as_mut() else {
            return TcpBridgeStatus::Disconnected(io::Error::new(
                io::ErrorKind::NotConnected,
                "not connected",
            ));
        };

        match framed.next().await {
            Some(Ok(msg)) => TcpBridgeStatus::Ok(msg),
            Some(Err(e)) => {
                self.framed = None;
                TcpBridgeStatus::Disconnected(e)
            }
            None => {
                self.framed = None;
                TcpBridgeStatus::Disconnected(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    "client closed connection",
                ))
            }
        }
    }

    pub async fn try_connect(&mut self) -> TcpBridgeStatus<()> {
        if self.framed.is_some() {
            return TcpBridgeStatus::Ok(());
        }

        match TcpStream::connect((Ipv4Addr::from_bits(self.addr), self.port)).await {
            Ok(sock) => {
                if let Err(e) = sock.set_nodelay(true) {
                    error!(error = %e, "failed to set TCP_NODELAY");
                    return TcpBridgeStatus::Disconnected(e);
                };
                info!("successfully connected to client");
                self.framed = Some(Framed::new(sock, MessageCodec));
                TcpBridgeStatus::Ok(())
            }
            Err(e) => TcpBridgeStatus::Disconnected(e),
        }
    }
}
