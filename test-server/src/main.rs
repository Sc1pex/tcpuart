use common::msg::{MAX_MESSAGE_LEN, MessageCodec, MessageControlRes};
use futures::{SinkExt, StreamExt};
use std::io;
use tokio::{
    io::{AsyncReadExt, stdin},
    net::{TcpListener, TcpStream},
    select,
    sync::broadcast,
};
use tokio_util::codec::Framed;

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:15113").await?;

    let (stdin_tx, stdin_rx) = broadcast::channel(128);

    tokio::spawn(async move {
        let mut buf = [0; MAX_MESSAGE_LEN];
        loop {
            let n = stdin().read(&mut buf).await.unwrap();
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

async fn handle_conn(conn: TcpStream, mut stdin_rx: broadcast::Receiver<String>) {
    let mut framed = Framed::new(conn, MessageCodec);
    let mut last_ctl_resp_status = MessageControlRes::NotSupported;

    loop {
        select! {
            Ok(input) = stdin_rx.recv() => {
                framed.send(input.as_bytes().into()).await.expect("failed to send data");
            }
            Some(msg) = framed.next() => {
                match msg {
                    Ok(common::msg::Message::Data(size, data)) => {
                        println!("Received data message: {:?}", String::from_utf8_lossy(&data[..size as usize]));
                    }
                    Ok(common::msg::Message::Config{ baudrate, data_bits, stop_bits, parity } ) => {
                        println!("Received config message: baudrate={}, data_bits={}, stop_bits={}, parity={:?}", baudrate, data_bits, stop_bits, parity);
                    }
                    Ok(common::msg::Message::ControlReq(cmd)) => {
                        println!("Received control request: cmd={:?}", cmd);
                        // For testing, we can just toggle the response status
                        last_ctl_resp_status = if last_ctl_resp_status == MessageControlRes::Ok {
                            MessageControlRes::NotSupported
                        } else {
                            MessageControlRes::Ok
                        };
                        framed.send(common::msg::Message::ControlRes(last_ctl_resp_status)).await.expect("failed to send control response");
                    }
                    Ok(common::msg::Message::ControlRes(status)) => {
                        eprintln!("Unexpected message type: ControlRes with status={:?}", status);
                    }
                    Err(e) => {
                        println!("Error receiving: {e}");
                    }
                }
            }
        }
    }
}
