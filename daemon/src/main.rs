use common::CtlMessage;
use std::{fs, io};
use tokio::net::{UnixListener, UnixStream};
use tokio::signal;
use tokio_stream::StreamExt;
use tokio_util::codec::{Decoder, FramedRead};

struct CtlMessageDecoder;

impl Decoder for CtlMessageDecoder {
    type Item = CtlMessage;
    type Error = io::Error;

    fn decode(
        &mut self,
        src: &mut tokio_util::bytes::BytesMut,
    ) -> Result<Option<Self::Item>, Self::Error> {
        CtlMessage::decode(src)
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let socket_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "./tcpuart.sock".to_string());

    let _ = fs::remove_file(&socket_path);

    let listener = UnixListener::bind(&socket_path).expect("Failed to bind to socket");

    loop {
        tokio::select! {
            res = listener.accept() => {
                match res {
                    Ok((stream, _)) => {
                        tokio::spawn(async move { handle_stream(stream).await });
                    }
                    Err(e) => eprintln!("Accept error: {}", e),
                }
            }
            _ = signal::ctrl_c() => {
                break;
            }
        }
    }

    println!("Cleaning up socket: {}", socket_path);
    let _ = fs::remove_file(&socket_path);
}

async fn handle_stream(stream: UnixStream) {
    let mut reader = FramedRead::new(stream, CtlMessageDecoder);

    while let Some(msg) = reader.next().await {
        match msg {
            Ok(msg) => println!("Got message: {:?}", msg),
            Err(e) => eprintln!("Received invalid message: {}", e),
        }
    }
}
