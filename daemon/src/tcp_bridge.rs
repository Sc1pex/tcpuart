use common::msg::{Message, MessageCodec};
use futures::{SinkExt, StreamExt};
use std::{io, net::Ipv4Addr, time::Duration};
use tokio::{net::TcpStream, time::timeout};
use tokio_util::codec::Framed;
use tracing::{error, info};

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

    pub async fn send(&mut self, msg: Message) -> io::Result<()> {
        if self.framed.is_none() {
            self.try_reconnect().await?;
        }
        match self.framed.as_mut().unwrap().send(msg).await {
            Ok(()) => Ok(()),
            Err(_) => {
                self.framed = None;
                self.try_reconnect().await?;
                self.framed.as_mut().unwrap().send(msg).await
            }
        }
    }

    pub async fn next(&mut self) -> io::Result<Message> {
        if self.framed.is_none() {
            self.try_reconnect().await?;
        }
        match self.framed.as_mut().unwrap().next().await {
            Some(Ok(msg)) => Ok(msg),
            Some(Err(_)) | None => {
                self.framed = None;
                self.try_reconnect().await?;
                match self.framed.as_mut().unwrap().next().await {
                    Some(Ok(msg)) => Ok(msg),
                    Some(Err(e)) => Err(e),
                    None => Err(io::Error::new(
                        io::ErrorKind::ConnectionAborted,
                        "connection closed",
                    )),
                }
            }
        }
    }
}

impl TcpBridge {
    async fn try_reconnect(&mut self) -> io::Result<()> {
        info!("attempting to connect to client");
        self.framed = None;
        const MAX_RECONNECT_TIME: Duration = Duration::from_secs(30);

        timeout(MAX_RECONNECT_TIME, async {
            let mut backoff = Duration::from_millis(50);

            loop {
                match TcpStream::connect((Ipv4Addr::from_bits(self.addr), self.port)).await {
                    Ok(sock) => {
                        if let Err(e) = sock.set_nodelay(true) {
                            error!(error = %e, "failed to set TCP_NODELAY");
                            return;
                        };
                        info!("successfully connected to client");
                        self.framed = Some(Framed::new(sock, MessageCodec));
                        return;
                    }
                    Err(e) => {
                        error!(
                            error = %e,
                            "failed to connect to client - retrying in {:?}",
                            backoff
                        );
                        tokio::time::sleep(backoff).await;
                        backoff *= 2;
                    }
                }
            }
        })
        .await
        .map_err(|_| {
            error!("failed to connect to client after {:?}", MAX_RECONNECT_TIME);
            io::Error::new(io::ErrorKind::TimedOut, "Failed to reconnect after 30s")
        })
    }
}
