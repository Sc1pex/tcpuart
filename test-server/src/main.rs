use std::{
    io::{ErrorKind, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex},
};

const MAX_MESSAGE_LEN: u16 = 1024;

#[repr(u16)]
#[derive(Debug, Clone, Copy)]
enum MessageKind {
    Data = 0,
    Config = 1,
}

impl TryFrom<u16> for MessageKind {
    type Error = &'static str;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Data),
            1 => Ok(Self::Config),
            _ => Err("Invalid message kind"),
        }
    }
}

#[repr(C)]
#[derive(Debug)]
struct MessageHeader {
    kind: MessageKind,
    size: u16,
}

#[derive(Debug)]
enum MessageHeaderError {
    InvalidKind,
    InvalidLength,
}

impl TryFrom<&[u8; 4]> for MessageHeader {
    type Error = MessageHeaderError;

    fn try_from(b: &[u8; 4]) -> Result<Self, Self::Error> {
        let kind = MessageKind::try_from(u16::from_be_bytes([b[0], b[1]]))
            .map_err(|_| MessageHeaderError::InvalidKind)?;
        let size = u16::from_be_bytes([b[2], b[3]]);
        if size > MAX_MESSAGE_LEN {
            return Err(MessageHeaderError::InvalidLength);
        }

        Ok(Self { kind, size })
    }
}

fn handle_client(stream: TcpStream, clients: Arc<Mutex<Vec<TcpStream>>>) -> std::io::Result<()> {
    let mut reader = std::io::BufReader::new(stream.try_clone()?);
    let mut header_buf: [u8; 4] = [0; 4];
    let mut message_buf: [u8; MAX_MESSAGE_LEN as usize] = [0; MAX_MESSAGE_LEN as usize];

    loop {
        if let Err(e) = reader.read_exact(&mut header_buf) {
            if e.kind() == ErrorKind::UnexpectedEof {
                println!("Client disconnected");
                break;
            }
            return Err(e);
        }

        let header = MessageHeader::try_from(&header_buf)
            .map_err(|e| std::io::Error::new(ErrorKind::InvalidData, format!("{e:?}")))?;

        println!("Got header: {header:?}");

        let msg_buf = &mut message_buf[..header.size as usize];
        reader.read_exact(msg_buf)?;

        if let Ok(s) = std::str::from_utf8(msg_buf) {
            println!("Got message: {}", s);
        } else {
            println!("Got message (bytes): {:?}", msg_buf);
        }
    }

    // Remove the client from the list when it disconnects
    let mut clients = clients.lock().unwrap();
    clients.retain(|c| {
        c.peer_addr()
            .map(|addr| addr != stream.peer_addr().unwrap())
            .unwrap_or(false)
    });

    Ok(())
}

fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:15113")?;
    let clients: Arc<Mutex<Vec<TcpStream>>> = Arc::new(Mutex::new(Vec::new()));

    // Thread for reading from stdin and broadcasting to all clients
    let stdin_clients = Arc::clone(&clients);
    std::thread::spawn(move || {
        let mut input = String::new();
        loop {
            input.clear();
            if std::io::stdin().read_line(&mut input).is_err() {
                eprintln!("Failed to read from stdin");
                break;
            }

            let message = input.trim_end().as_bytes();
            if message.is_empty() {
                continue;
            }

            let size = message.len() as u16;
            if size > MAX_MESSAGE_LEN {
                eprintln!("Message too long to send");
                continue;
            }

            let mut header = [0u8; 4];
            header[0..2].copy_from_slice(&(MessageKind::Data as u16).to_be_bytes());
            header[2..4].copy_from_slice(&size.to_be_bytes());

            let mut clients = stdin_clients.lock().unwrap();
            let mut disconnected = Vec::new();

            for (i, client) in clients.iter_mut().enumerate() {
                if let Err(e) = client.write_all(&header).and_then(|_| client.write_all(message)) {
                    eprintln!("Failed to send to client: {}", e);
                    disconnected.push(i);
                }
            }

            // Clean up disconnected clients in reverse order
            for i in disconnected.into_iter().rev() {
                clients.remove(i);
            }
        }
    });

    println!("Listening on 127.0.0.1:15113...");

    for stream in listener.incoming() {
        let stream = stream?;
        println!("Got new connection from {}", stream.peer_addr()?);

        let mut clients_lock = clients.lock().unwrap();
        clients_lock.push(stream.try_clone()?);
        drop(clients_lock);

        let clients_handle = Arc::clone(&clients);
        std::thread::spawn(move || {
            if let Err(e) = handle_client(stream, clients_handle) {
                eprintln!("Handler error: {e}");
            }
        });
    }

    Ok(())
}
