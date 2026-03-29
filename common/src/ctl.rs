use std::io;
use tokio_util::{
    bytes::{Buf, BufMut, BytesMut},
    codec::{Decoder, Encoder},
};

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

pub enum CtlResponse {
    AddOk(String),
    RemoveOk,
    List(Vec<ConnectionInfo>),
    Error(String),
}

impl CtlMessage {
    pub fn msg_type(&self) -> u8 {
        match self {
            CtlMessage::Add { .. } => 1,
            CtlMessage::Remove { .. } => 2,
            CtlMessage::List => 3,
        }
    }
}

impl CtlResponse {
    pub fn msg_type(&self) -> u8 {
        match self {
            CtlResponse::AddOk(_) => 1,
            CtlResponse::RemoveOk => 2,
            CtlResponse::List(_) => 3,
            CtlResponse::Error(_) => 255,
        }
    }
}

pub struct CtlMessageEncoder;
pub struct CtlMessageDecoder;
pub struct CtlResponseEncoder;
pub struct CtlResponseDecoder;

impl Encoder<CtlMessage> for CtlMessageEncoder {
    type Error = io::Error;

    fn encode(&mut self, item: CtlMessage, dst: &mut BytesMut) -> Result<(), Self::Error> {
        dst.put_u8(item.msg_type());

        match item {
            CtlMessage::Add { name, addr, port } => {
                encode_str(&name, dst)?;
                dst.put_u32(addr);
                dst.put_u16(port);
            }
            CtlMessage::Remove { name } => {
                encode_str(&name, dst)?;
            }
            CtlMessage::List => {}
        }
        Ok(())
    }
}

impl Decoder for CtlMessageDecoder {
    type Item = CtlMessage;
    type Error = io::Error;

    fn decode(
        &mut self,
        src: &mut tokio_util::bytes::BytesMut,
    ) -> Result<Option<Self::Item>, Self::Error> {
        let mut cursor = io::Cursor::new(&mut *src);

        let Ok(message_kind) = cursor.try_get_u8() else {
            return Ok(None);
        };

        let msg = match message_kind {
            1 => {
                let Some(name) = decode_str(&mut cursor)? else {
                    return Ok(None);
                };
                let Ok(addr) = cursor.try_get_u32() else {
                    return Ok(None);
                };
                let Ok(port) = cursor.try_get_u16() else {
                    return Ok(None);
                };
                CtlMessage::Add { name, addr, port }
            }
            2 => {
                let Some(name) = decode_str(&mut cursor)? else {
                    return Ok(None);
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

impl Encoder<CtlResponse> for CtlResponseEncoder {
    type Error = io::Error;

    fn encode(
        &mut self,
        item: CtlResponse,
        dst: &mut tokio_util::bytes::BytesMut,
    ) -> Result<(), Self::Error> {
        dst.put_u8(item.msg_type());
        match item {
            CtlResponse::AddOk(pts_path) => {
                encode_str(&pts_path, dst)?;
            }
            CtlResponse::RemoveOk => {}
            CtlResponse::List(connections) => {
                dst.put_u16(connections.len() as u16);
                for conn in connections {
                    encode_str(&conn.name, dst)?;
                    dst.put_u32(conn.addr);
                    dst.put_u16(conn.port);
                    encode_str(&conn.pts_path, dst)?;
                }
            }
            CtlResponse::Error(msg) => {
                encode_str(&msg, dst)?;
            }
        }
        Ok(())
    }
}

impl Decoder for CtlResponseDecoder {
    type Item = CtlResponse;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let mut cursor = io::Cursor::new(&mut *src);

        let Ok(message_kind) = cursor.try_get_u8() else {
            return Ok(None);
        };

        let response = match message_kind {
            1 => {
                let Some(pts_path) = decode_str(&mut cursor)? else {
                    return Ok(None);
                };
                CtlResponse::AddOk(pts_path)
            }
            2 => CtlResponse::RemoveOk,
            3 => {
                let Ok(count) = cursor.try_get_u16() else {
                    return Ok(None);
                };
                let mut connections = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    let Some(name) = decode_str(&mut cursor)? else {
                        return Ok(None);
                    };
                    let Ok(addr) = cursor.try_get_u32() else {
                        return Ok(None);
                    };
                    let Ok(port) = cursor.try_get_u16() else {
                        return Ok(None);
                    };
                    let Some(pts_path) = decode_str(&mut cursor)? else {
                        return Ok(None);
                    };
                    connections.push(ConnectionInfo {
                        name,
                        addr,
                        port,
                        pts_path,
                    });
                }
                CtlResponse::List(connections)
            }
            255 => {
                let Some(msg) = decode_str(&mut cursor)? else {
                    return Ok(None);
                };
                CtlResponse::Error(msg)
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unknown response type",
                ))
            }
        };

        // Advance the original buffer by the number of bytes read
        let bytes_read = cursor.position() as usize;
        src.advance(bytes_read);
        Ok(Some(response))
    }
}

fn encode_str(s: &str, dst: &mut BytesMut) -> io::Result<()> {
    let size = s.len();
    if size > 255 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "string too long",
        ));
    }
    dst.put_u8(size as u8);
    dst.put_slice(s.as_bytes());
    Ok(())
}

fn decode_str(cursor: &mut io::Cursor<&mut BytesMut>) -> io::Result<Option<String>> {
    let size = match cursor.try_get_u8() {
        Ok(size) => size as usize,
        Err(_) => return Ok(None),
    };

    let mut buf = vec![0; size];
    if cursor.try_copy_to_slice(&mut buf).is_err() {
        return Ok(None);
    }

    Ok(Some(String::from_utf8(buf).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidData, "invalid string")
    })?))
}
