use std::io::Write;
use std::net::Ipv4Addr;
use std::os::unix::net::UnixStream;

use bytes::BytesMut;
use clap::{Parser, Subcommand};
use common::CtlMessage;

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
}

fn main() {
    let cli = Cli::parse();

    let mut conn = match UnixStream::connect(cli.socket) {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("Failed to connect to socket: {}", e);
            return;
        }
    };

    let message = match cli.command {
        Command::Add { name, addr, port } => {
            let addr: Ipv4Addr = addr.parse().unwrap_or_else(|_| {
                eprintln!("Invalid IP address: {}", addr);
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
    };

    let mut buf = BytesMut::new();
    if let Err(e) = message.encode(&mut buf) {
        eprintln!("Failed to encode message: {}", e);
        return;
    }

    if let Err(e) = conn.write_all(&buf) {
        eprintln!("Failed to write to socket: {}", e);
    }
}
