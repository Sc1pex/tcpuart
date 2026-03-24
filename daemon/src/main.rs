use common::{CtlMessage, CtlResponse};
use futures::SinkExt;
use state::State;
use std::{fs, io};
use tokio::{
    net::{UnixListener, UnixStream},
    signal,
    sync::{mpsc, oneshot},
};
use tokio_stream::StreamExt;
use tokio_util::codec::{Decoder, Encoder, FramedRead, FramedWrite};

mod async_pty;
mod state;

#[tokio::main(flavor = "current_thread")]
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
                        tokio::spawn(async move { handle_stream(stream, msg_tx).await });
                    }
                    Err(e) => eprintln!("Accept error: {}", e),
                }
            }
            Some((msg, send)) = msg_rx.recv() => {
                let resp = state.handle_msg(msg);
                let _ = send.send(resp);
            }
            _ = signal::ctrl_c() => {
                break;
            }
        }
    }

    println!("Cleaning up socket: {}", socket_path);
    let _ = fs::remove_file(&socket_path);
}

struct CtlMessageDecoder;
struct CtlResponseEncoder;

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

impl Encoder<CtlResponse> for CtlResponseEncoder {
    type Error = io::Error;

    fn encode(
        &mut self,
        item: CtlResponse,
        dst: &mut tokio_util::bytes::BytesMut,
    ) -> Result<(), Self::Error> {
        item.encode(dst)
    }
}

async fn handle_stream(
    mut stream: UnixStream,
    msg_tx: mpsc::Sender<(CtlMessage, oneshot::Sender<CtlResponse>)>,
) {
    let (reader, writer) = stream.split();
    let mut reader = FramedRead::new(reader, CtlMessageDecoder);
    let mut writer = FramedWrite::new(writer, CtlResponseEncoder);

    while let Some(msg) = reader.next().await {
        match msg {
            Ok(msg) => {
                let (tx, rx) = oneshot::channel();
                if msg_tx.send((msg, tx)).await.is_err() {
                    // Main tasked closed
                    return;
                }
                let resp = match rx.await {
                    Ok(resp) => resp,
                    Err(_) => CtlResponse::Error("Something went wrong".to_string()),
                };
                writer
                    .send(resp)
                    .await
                    .expect("Failed to send message to user");
            }
            Err(e) => eprintln!("Received invalid message: {}", e),
        }
    }
}
