use clap::{Parser, Subcommand};
use common::ctl::{CtlMessage, CtlMessageEncoder, CtlResponse, CtlResponseDecoder};
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

    let mut conn = match UnixStream::connect(cli.socket).await {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("Failed to connect to socket: {e}");
            return;
        }
    };
    let (reader, writer) = conn.split();
    let mut writer = FramedWrite::new(writer, CtlMessageEncoder);
    let mut reader = FramedRead::new(reader, CtlResponseDecoder);

    let message = match cli.command {
        Command::Add { name, addr, port } => {
            let addr: Ipv4Addr = addr.parse().unwrap_or_else(|_| {
                eprintln!("Invalid IP address: {addr}");
                std::process::exit(1);
            });
            CtlMessage::Add {
                name,
                addr: u32::from(addr),
                port,
            }
        }
        Command::Remove { name } => CtlMessage::Remove { name },
        Command::List => CtlMessage::List,
        Command::Reset { name } => CtlMessage::Reset { name },
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

fn handle_response(resp: CtlResponse) {
    match resp {
        CtlResponse::AddOk(pts_path) => {
            println!("Successfully added connection. PTY device: {pts_path}");
        }
        CtlResponse::Error(msg) => {
            eprintln!("Error from daemon: {msg}");
        }
        CtlResponse::RemoveOk => {
            println!("Successfully removed connection");
        }
        CtlResponse::List(l) => {
            if l.is_empty() {
                println!("No active connections");
            } else {
                println!("Active connections:");
                for info in l {
                    let ip = Ipv4Addr::from(info.addr);
                    println!(
                        "- {}: {}:{} (PTY: {})",
                        info.name, ip, info.port, info.pts_path
                    );
                }
            }
        }
        CtlResponse::ResetOk => {
            println!("Successfully reset device");
        }
    }
}
