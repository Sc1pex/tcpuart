use common::CtlMessage;
use state::State;
use std::{fs, io};
use tokio::{
    net::{UnixListener, UnixStream},
    signal,
    sync::mpsc,
};
use tokio_stream::StreamExt;
use tokio_util::codec::{Decoder, FramedRead};

mod state;

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

#[allow(unexpected_cfgs)]
#[tokio::main(flavor = "local")]
async fn main() {
    let socket_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "./tcpuart.sock".to_string());
    let _ = fs::remove_file(&socket_path);

    let mut state = State::default();
    let (msg_tx, mut msg_rx) = mpsc::channel(128);

    let listener = UnixListener::bind(&socket_path).expect("Failed to bind to socket");
    loop {
        tokio::select! {
            res = listener.accept() => {
                match res {
                    Ok((stream, _)) => {
                        let msg_tx = msg_tx.clone();
                        tokio::task::spawn_local(async move { handle_stream(stream, msg_tx).await });
                    }
                    Err(e) => eprintln!("Accept error: {}", e),
                }
            }
            Some(msg) = msg_rx.recv() => {
                if let Err(e) = state.handle_msg(msg) {
                    eprintln!("Error handling message: {e}");
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

async fn handle_stream(stream: UnixStream, msg_tx: mpsc::Sender<CtlMessage>) {
    let mut reader = FramedRead::new(stream, CtlMessageDecoder);

    while let Some(msg) = reader.next().await {
        match msg {
            Ok(msg) => {
                let _ = msg_tx.send(msg).await;
            }
            Err(e) => eprintln!("Received invalid message: {}", e),
        }
    }
}
