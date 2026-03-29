use async_pty::AsyncPty;
use common::ctl::{CtlMessage, CtlMessageDecoder, CtlResponse, CtlResponseEncoder};
use connection::{conn_task, Connection};
use futures::{SinkExt, StreamExt};
use nix::{fcntl::OFlag, pty};
use state::State;
use std::{fs, net::Ipv4Addr};
use tokio::{
    net::{TcpStream, UnixListener, UnixStream},
    signal,
    sync::{mpsc, oneshot},
};
use tokio_util::codec::{FramedRead, FramedWrite};

mod async_pty;
mod connection;
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
                    Err(e) => eprintln!("Accept error: {e}"),
                }
            }
            Some((msg, send)) = msg_rx.recv() => {
                let resp = handle_message(&mut state, msg).await;
                let _ = send.send(resp);
            }
            _ = signal::ctrl_c() => {
                break;
            }
        }
    }

    println!("Cleaning up socket: {socket_path}");
    let _ = fs::remove_file(&socket_path);
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
            Err(e) => eprintln!("Received invalid message: {e}"),
        }
    }
}

async fn handle_message(state: &mut State, msg: CtlMessage) -> CtlResponse {
    match msg {
        CtlMessage::Add { name, addr, port } => {
            if state.exists(&name) {
                return CtlResponse::Error(format!("Connection with name '{name}' already exists"));
            }

            let master = match pty::posix_openpt(OFlag::O_RDWR | OFlag::O_NOCTTY) {
                Ok(master) => master,
                Err(e) => return CtlResponse::Error(format!("Failed to create pty: {e}")),
            };
            if let Err(e) = pty::grantpt(&master) {
                return CtlResponse::Error(format!("Failed to grant pty: {e}"));
            }
            if let Err(e) = pty::unlockpt(&master) {
                return CtlResponse::Error(format!("Failed to unlock pty: {e}"));
            }

            let slave_name = match unsafe { pty::ptsname(&master) } {
                Ok(name) => name,
                Err(e) => return CtlResponse::Error(format!("Failed to get pts name: {e}")),
            };

            let master = match AsyncPty::new(master) {
                Ok(master) => master,
                Err(e) => {
                    eprintln!("Failed to create async pty: {e}");
                    return CtlResponse::Error("Something went wrong".into());
                }
            };

            let stream = match TcpStream::connect((Ipv4Addr::from_bits(addr), port)).await {
                Ok(stream) => stream,
                Err(e) => {
                    return CtlResponse::Error(format!(
                        "Failed to connect to {}:{port} - {e}",
                        Ipv4Addr::from_bits(addr)
                    ));
                }
            };

            let (conn, shutdown_rx) = Connection::new(name, addr, port, slave_name.clone());
            state.add(conn);

            tokio::spawn(conn_task(master, stream, shutdown_rx));
            CtlResponse::AddOk(slave_name)
        }
        CtlMessage::Remove { name } => {
            if state.remove(&name) {
                CtlResponse::RemoveOk
            } else {
                CtlResponse::Error(format!("No connection found with name: {name}"))
            }
        }
        CtlMessage::List => CtlResponse::List(state.list()),
    }
}
