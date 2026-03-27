use common::msg::{decode_message, encode_message, MessageHeader, MAX_MESSAGE_LEN};
use std::io;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    select,
    sync::broadcast,
};
use tokio_stream::StreamExt;
use tokio_util::{
    bytes::BytesMut,
    codec::{Decoder, FramedRead},
};

#[tokio::main]
async fn main() -> io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:15113").await?;

    let (stdin_tx, stdin_rx) = broadcast::channel(128);

    tokio::spawn(async move {
        let mut buf = [0; MAX_MESSAGE_LEN as usize];
        loop {
            let n = tokio::io::stdin().read(&mut buf).await.unwrap();
            if n == 0 {
                break;
            }
            let input = String::from_utf8_lossy(&buf[..n]).to_string();
            stdin_tx.send(input).unwrap();
        }
    });

    loop {
        let (conn, _) = listener.accept().await?;
        let stdin_rx = stdin_rx.resubscribe();
        tokio::spawn(handle_conn(conn, stdin_rx));
    }
}

pub struct MessageDecoder;

impl Decoder for MessageDecoder {
    type Item = (MessageHeader, Vec<u8>);
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let mut data = vec![0; MAX_MESSAGE_LEN as usize];
        let header = decode_message(src, data.as_mut_slice())?;
        Ok(header.map(|h| (h, data)))
    }
}

async fn handle_conn(mut conn: TcpStream, mut stdin_rx: broadcast::Receiver<String>) {
    let mut buf = BytesMut::new();

    let (reader, mut writer) = conn.split();
    let mut stream = FramedRead::new(reader, MessageDecoder);
    loop {
        select! {
            Ok(input) = stdin_rx.recv() => {
                buf.clear();
                encode_message(MessageHeader::data(input.len() as u8), input.as_bytes(), &mut buf).unwrap();
                writer.write_all(&buf).await.unwrap();
            }
            Some(msg) = stream.next() => {
                match msg {
                    Ok((header, data)) => {
                        println!("Received message: header={:?}, data={:?}", header, String::from_utf8_lossy(&data[..header.size as usize]));
                    }
                    Err(e) => {
                        println!("Error receiving: {e}");
                    }
                }
            }
        }
    }
}
