use async_pty::AsyncPty;
use clap::Parser;
use common::{
    ctl::{CtlMessage, CtlMessageDecoder, CtlResponse, CtlResponseEncoder},
    msg::{MessageControlReq, MessageControlRes},
};
use connection::Connection;
use event::DaemonEvent;
use futures::{SinkExt, StreamExt};
use nix::{fcntl::OFlag, pty};
use state::State;
use std::fs;
use tokio::{
    net::{UnixListener, UnixStream},
    signal,
    sync::{mpsc, oneshot},
};
use tokio_util::codec::{FramedRead, FramedWrite};
use tracing::{error, info, instrument};
use tracing_subscriber::{EnvFilter, fmt};

mod async_pty;
mod connection;
mod event;
mod state;
mod tcp_bridge;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the unix socket
    #[arg(short, long, default_value = "./tcpuart.sock")]
    socket: String,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    fmt().with_env_filter(EnvFilter::from_default_env()).init();

    let args = Args::parse();
    let socket_path = args.socket;
    let _ = fs::remove_file(&socket_path);

    let mut state = State::default();
    let (event_tx, mut event_rx) = mpsc::channel(128);

    let listener = UnixListener::bind(&socket_path).expect("Failed to bind to socket");
    info!(
        path = socket_path,
        "daemon started and listening for connections"
    );
    loop {
        tokio::select! {
            res = listener.accept() => {
                match res {
                    Ok((stream, _)) => {
                        let event_tx = event_tx.clone();
                        tokio::spawn(async move { handle_stream(stream, event_tx).await });
                    }
                    Err(e) => error!(error = %e, "accept failed"),
                }
            }
            Some(event) = event_rx.recv() => {
                match event {
                    DaemonEvent::CliCommand(ctl_message, sender) => {
                        let resp = handle_ctl_message(&mut state, ctl_message, event_tx.clone()).await;
                        let _ = sender.send(resp);
                    }
                    DaemonEvent::ConnectionClosed(name) => {
                        info!(name, "connection closed, removing from state");
                        state.remove(&name);
                    }
                }
            }
            _ = signal::ctrl_c() => {
                break;
            }
        }
    }

    info!(path = socket_path, "cleaning up socket");
    let _ = fs::remove_file(&socket_path);
}

async fn handle_stream(mut stream: UnixStream, event_tx: mpsc::Sender<DaemonEvent>) {
    let (reader, writer) = stream.split();
    let mut reader = FramedRead::new(reader, CtlMessageDecoder);
    let mut writer = FramedWrite::new(writer, CtlResponseEncoder);

    while let Some(msg) = reader.next().await {
        match msg {
            Ok(msg) => {
                let (tx, rx) = oneshot::channel();
                if event_tx
                    .send(DaemonEvent::CliCommand(msg, tx))
                    .await
                    .is_err()
                {
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
            Err(e) => {
                error!(error = %e, "failed to decode message from user");
                break;
            }
        }
    }
}

#[instrument(skip(state, event_tx))]
async fn handle_ctl_message(
    state: &mut State,
    msg: CtlMessage,
    event_tx: mpsc::Sender<DaemonEvent>,
) -> CtlResponse {
    match msg {
        CtlMessage::Add { name, addr, port } => {
            if state.exists(&name) {
                return CtlResponse::Error(format!("Connection with name '{name}' already exists"));
            }

            let master = match pty::posix_openpt(OFlag::O_RDWR | OFlag::O_NOCTTY) {
                Ok(master) => master,
                Err(e) => {
                    error!(error = %e, "failed to open pty master");
                    return CtlResponse::Error(format!(
                        "Failed to create pty. See daemon logs for details"
                    ));
                }
            };
            if let Err(e) = pty::grantpt(&master) {
                error!(error = %e, "failed to grant pty");
                return CtlResponse::Error(format!(
                    "Failed to grant pty. See daemon logs for details"
                ));
            }
            if let Err(e) = pty::unlockpt(&master) {
                error!(error = %e, "failed to unlock pty");
                return CtlResponse::Error(format!(
                    "Failed to unlock pty. See daemon logs for details"
                ));
            }

            let slave_name = match unsafe { pty::ptsname(&master) } {
                Ok(name) => name,
                Err(e) => {
                    error!(error = %e, "failed to get pts name");
                    return CtlResponse::Error(format!(
                        "Failed to get pts name. See daemon logs for details"
                    ));
                }
            };

            let master = match AsyncPty::new(master) {
                Ok(master) => master,
                Err(e) => {
                    error!(error = %e, "failed to create AsyncPty");
                    return CtlResponse::Error(
                        "Failed to create AsyncPty. See daemon logs for details".to_string(),
                    );
                }
            };

            let conn = Connection::build_and_spawn(
                name.clone(),
                addr,
                port,
                master,
                slave_name.clone(),
                event_tx,
            );
            state.add(conn);
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
        CtlMessage::Reset { name } => {
            match state.send_ctl_msg(&name, MessageControlReq::Reset).await {
                Some(resp) => match resp {
                    MessageControlRes::Ok => CtlResponse::ResetOk,
                    MessageControlRes::NotSupported => CtlResponse::Error(format!(
                        "Connection '{name}' does not support reset command"
                    )),
                },
                None => CtlResponse::Error(format!("No connection found with name: {name}")),
            }
        }
    }
}
