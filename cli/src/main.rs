use clap::{Parser, Subcommand};
use common::ctl::{DaemonRequest, DaemonRequestEncoder, DaemonResponse, DaemonResponseDecoder};
use futures::{SinkExt, StreamExt};
use std::net::Ipv4Addr;
use tokio::net::UnixStream;
use tokio_util::codec::{FramedRead, FramedWrite};

#[derive(Parser)]
struct Cli {
    #[arg(short, long, default_value = "./tcpuart.sock")]
    socket: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Add a new connection
    Add {
        name: String,
        addr: String,
        #[arg(default_value = "15113")]
        port: u16,
    },
    /// Remove a connection
    Remove { name: String },
    /// List active connections
    List,
    /// Sends a reset signal to the connected device (requires support from the server)
    Reset { name: String },
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let cli = Cli::parse();

    let conn = match UnixStream::connect(cli.socket).await {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("Failed to connect to socket: {e}");
            return;
        }
    };
    let (reader, writer) = conn.into_split();
    let mut writer = FramedWrite::new(writer, DaemonRequestEncoder);
    let mut reader = FramedRead::new(reader, DaemonResponseDecoder);

    let message = match cli.command {
        Command::Add { name, addr, port } => {
            let addr: Ipv4Addr = addr.parse().unwrap_or_else(|_| {
                eprintln!("Invalid IP address: {addr}");
                std::process::exit(1);
            });
            DaemonRequest::Add {
                name,
                addr: u32::from(addr),
                port,
            }
        }
        Command::Remove { name } => DaemonRequest::Remove { name },
        Command::List => DaemonRequest::List,
        Command::Reset { name } => DaemonRequest::Reset { name },
    };

    if let Err(e) = writer.send(message).await {
        eprintln!("Failed to send message to socket: {e}");
        return;
    }

    match reader.next().await {
        Some(Ok(resp)) => handle_response(resp),
        Some(Err(e)) => eprintln!("Failed to decode daemon response: {e}"),
        None => eprintln!("Daemon closed connection unexpectedly"),
    }
}

fn handle_response(resp: DaemonResponse) {
    match resp {
        DaemonResponse::AddOk(pts_path) => {
            println!("Successfully added connection. PTY device: {pts_path}");
        }
        DaemonResponse::Error(msg) => {
            eprintln!("Error from daemon: {msg}");
        }
        DaemonResponse::RemoveOk => {
            println!("Successfully removed connection");
        }
        DaemonResponse::List(l) => {
            if l.is_empty() {
                println!("No active connections");
            } else {
                println!("Active connections:");
                for info in l {
                    let ip = Ipv4Addr::from(info.addr);
                    let status = if info.connected {
                        "[CONNECTED]"
                    } else {
                        "[OFFLINE]"
                    };
                    println!(
                        "- {}: {}:{} (PTY: {}) {}",
                        info.name, ip, info.port, info.pts_path, status
                    );
                }
            }
        }
        DaemonResponse::ResetOk => {
            println!("Successfully reset device");
        }
    }
}
