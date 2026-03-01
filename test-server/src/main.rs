use std::{
    io::{ErrorKind, Read},
    net::{TcpListener, TcpStream},
};

const MAX_MESSAGE_LEN: u16 = 1024;

#[repr(u16)]
#[derive(Debug)]
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

#[repr(C)]
#[derive(Debug)]
#[allow(dead_code)]
struct DataMessage {
    buf: Box<[u8]>,
}

#[repr(C)]
#[derive(Debug)]
#[allow(dead_code)]
struct ConfigMessage {
    baud_rate: u32,
}

fn handle_client(stream: TcpStream) -> std::io::Result<()> {
    let mut reader = std::io::BufReader::new(stream);
    let mut header_buf: [u8; std::mem::size_of::<MessageHeader>()] =
        [0; std::mem::size_of::<MessageHeader>()];
    let mut message_buf: [u8; MAX_MESSAGE_LEN as usize] = [0; MAX_MESSAGE_LEN as usize];

    loop {
        reader.read_exact(&mut header_buf)?;
        let header = MessageHeader::try_from(&header_buf)
            .map_err(|e| std::io::Error::new(ErrorKind::InvalidData, format!("{e:?}")))?;

        println!("Got header: {header:?}");

        let msg_buf = &mut message_buf[..header.size as usize];
        reader.read_exact(msg_buf)?;

        println!("Got message: {:?}", msg_buf);
    }
}

fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:15113")?;

    for stream in listener.incoming() {
        let stream = stream?;
        println!("Got new connection");

        std::thread::spawn(move || {
            if let Err(e) = handle_client(stream) {
                eprintln!("Handler error: {e}");
            }
        });
    }

    Ok(())
}
