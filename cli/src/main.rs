use std::io::{Read, Write};
use std::net::Ipv4Addr;
use std::os::unix::net::UnixStream;

use bytes::BytesMut;
use clap::{Parser, Subcommand};
use common::{CtlMessage, CtlResponse};

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
        return;
    }
    buf.clear();

    let mut temp_buf = [0u8; 1024];
    loop {
        let n = match conn.read(&mut temp_buf) {
            Ok(0) => {
                eprintln!("Daemon closed connection unexpectedly");
                return;
            }
            Ok(n) => n,
            Err(e) => {
                eprintln!("Failed to read from socket: {}", e);
                return;
            }
        };

        buf.extend_from_slice(&temp_buf[..n]);

        match CtlResponse::decode(&mut buf) {
            Ok(Some(response)) => {
                handle_response(response);
                break;
            }
            Ok(None) => continue,
            Err(e) => {
                eprintln!("Failed to decode response: {}", e);
                return;
            }
        }
    }
}

fn handle_response(resp: CtlResponse) {
    match resp {
        CtlResponse::AddOk(pts_path) => {
            println!("Successfully added connection. PTY device: {}", pts_path);
        }
        CtlResponse::Error(msg) => {
            eprintln!("Error from daemon: {}", msg);
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
    }
}
