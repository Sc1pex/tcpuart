use bytes::{Buf, BufMut, BytesMut};
use codec::encode_str;
use std::io;

mod codec;

#[derive(Debug)]
pub enum CtlMessage {
    Add { name: String, addr: u32, port: u16 },
    Remove { name: String },
    List,
}

pub struct ConnectionInfo {
    pub name: String,
    pub addr: u32,
    pub port: u16,
    pub pts_path: String,
}

impl CtlMessage {
    pub fn msg_type(&self) -> u8 {
        match self {
            CtlMessage::Add { .. } => 1,
            CtlMessage::Remove { .. } => 2,
            CtlMessage::List => 3,
        }
    }

    pub fn encode(&self, dst: &mut BytesMut) -> io::Result<()> {
        dst.put_u8(self.msg_type());

        match self {
            CtlMessage::Add { name, addr, port } => {
                encode_str(name, dst)?;
                dst.put_u32(*addr);
                dst.put_u16(*port);
            }
            CtlMessage::Remove { name } => {
                encode_str(name, dst)?;
            }
            CtlMessage::List => {}
        }
        Ok(())
    }

    pub fn decode(src: &mut BytesMut) -> io::Result<Option<Self>> {
        let mut cursor = io::Cursor::new(&mut *src);

        let message_kind = match cursor.try_get_u8() {
            Ok(kind) => kind,
            Err(_) => return Ok(None),
        };

        let msg = match message_kind {
            1 => {
                let name = match codec::decode_str(&mut cursor)? {
                    Some(name) => name,
                    None => return Ok(None),
                };
                let addr = match cursor.try_get_u32() {
                    Ok(addr) => addr,
                    Err(_) => return Ok(None),
                };
                let port = match cursor.try_get_u16() {
                    Ok(port) => port,
                    Err(_) => return Ok(None),
                };
                CtlMessage::Add { name, addr, port }
            }
            2 => {
                let name = match codec::decode_str(&mut cursor)? {
                    Some(name) => name,
                    None => return Ok(None),
                };
                CtlMessage::Remove { name }
            }
            3 => CtlMessage::List,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unknown message type",
                ))
            }
        };

        // Advance the original buffer by the number of bytes read
        let bytes_read = cursor.position() as usize;
        src.advance(bytes_read);
        Ok(Some(msg))
    }
}
