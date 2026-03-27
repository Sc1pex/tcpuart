use common::msg::{MessageDecoder, MessageEncoder, MAX_MESSAGE_LEN};
use futures::SinkExt;
use std::io;
use tokio::{
    io::AsyncReadExt,
    net::{TcpListener, TcpStream},
    select,
    sync::broadcast,
};
use tokio_stream::StreamExt;
use tokio_util::codec::{FramedRead, FramedWrite};

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

async fn handle_conn(mut conn: TcpStream, mut stdin_rx: broadcast::Receiver<String>) {
    let (reader, writer) = conn.split();
    let mut reader = FramedRead::new(reader, MessageDecoder);
    let mut writer = FramedWrite::new(writer, MessageEncoder);

    loop {
        select! {
            Ok(input) = stdin_rx.recv() => {
                writer.send(input.as_bytes().into()).await.expect("failed to send data");
            }
            Some(msg) = reader.next() => {
                match msg {
                    Ok(common::msg::Message::Data(size, data)) => {
                        println!("Received data message: {:?}", String::from_utf8_lossy(&data[..size as usize]));
                    }
                    Ok(common::msg::Message::Config{} ) => {
                        println!("Received config message");
                    }
                    Err(e) => {
                        println!("Error receiving: {e}");
                    }
                }
            }
        }
    }
}
