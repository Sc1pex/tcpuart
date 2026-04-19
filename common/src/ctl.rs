use std::io;
use tokio_util::{
    bytes::{Buf, BufMut, BytesMut},
    codec::{Decoder, Encoder},
};

/// Commands sent from the CLI to the Daemon over the Unix domain socket
#[derive(Debug)]
pub enum DaemonRequest {
    Add { name: String, addr: u32, port: u16 },
    Remove { name: String },
    List,
    Reset { name: String },
}

pub struct ConnectionInfo {
    pub name: String,
    pub addr: u32,
    pub port: u16,
    pub pts_path: String,
}

/// Responses sent from the Daemon back to the CLI
pub enum DaemonResponse {
    AddOk(String),
    RemoveOk,
    List(Vec<ConnectionInfo>),
    ResetOk,
    Error(String),
}

impl DaemonRequest {
    pub fn msg_type(&self) -> u8 {
        match self {
            DaemonRequest::Add { .. } => 1,
            DaemonRequest::Remove { .. } => 2,
            DaemonRequest::List => 3,
            DaemonRequest::Reset { .. } => 4,
        }
    }
}

impl DaemonResponse {
    pub fn msg_type(&self) -> u8 {
        match self {
            DaemonResponse::AddOk(_) => 1,
            DaemonResponse::RemoveOk => 2,
            DaemonResponse::List(_) => 3,
            DaemonResponse::ResetOk => 4,
            DaemonResponse::Error(_) => 255,
        }
    }
}

pub struct DaemonRequestEncoder;
pub struct DaemonRequestDecoder;
pub struct DaemonResponseEncoder;
pub struct DaemonResponseDecoder;

impl Encoder<DaemonRequest> for DaemonRequestEncoder {
    type Error = io::Error;

    fn encode(&mut self, item: DaemonRequest, dst: &mut BytesMut) -> Result<(), Self::Error> {
        dst.put_u8(item.msg_type());

        match item {
            DaemonRequest::Add { name, addr, port } => {
                encode_str(&name, dst)?;
                dst.put_u32(addr);
                dst.put_u16(port);
            }
            DaemonRequest::Remove { name } => {
                encode_str(&name, dst)?;
            }
            DaemonRequest::List => {}
            DaemonRequest::Reset { name } => {
                encode_str(&name, dst)?;
            }
        }
        Ok(())
    }
}

impl Decoder for DaemonRequestDecoder {
    type Item = DaemonRequest;
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
                DaemonRequest::Add { name, addr, port }
            }
            2 => {
                let Some(name) = decode_str(&mut cursor)? else {
                    return Ok(None);
                };
                DaemonRequest::Remove { name }
            }
            3 => DaemonRequest::List,
            4 => {
                let Some(name) = decode_str(&mut cursor)? else {
                    return Ok(None);
                };
                DaemonRequest::Reset { name }
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unknown message type",
                ));
            }
        };

        // Advance the original buffer by the number of bytes read
        let bytes_read = cursor.position() as usize;
        src.advance(bytes_read);
        Ok(Some(msg))
    }
}

impl Encoder<DaemonResponse> for DaemonResponseEncoder {
    type Error = io::Error;

    fn encode(
        &mut self,
        item: DaemonResponse,
        dst: &mut tokio_util::bytes::BytesMut,
    ) -> Result<(), Self::Error> {
        dst.put_u8(item.msg_type());
        match item {
            DaemonResponse::AddOk(pts_path) => {
                encode_str(&pts_path, dst)?;
            }
            DaemonResponse::RemoveOk => {}
            DaemonResponse::List(connections) => {
                dst.put_u16(connections.len() as u16);
                for conn in connections {
                    encode_str(&conn.name, dst)?;
                    dst.put_u32(conn.addr);
                    dst.put_u16(conn.port);
                    encode_str(&conn.pts_path, dst)?;
                }
            }
            DaemonResponse::ResetOk => {}
            DaemonResponse::Error(msg) => {
                encode_str(&msg, dst)?;
            }
        }
        Ok(())
    }
}

impl Decoder for DaemonResponseDecoder {
    type Item = DaemonResponse;
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
                DaemonResponse::AddOk(pts_path)
            }
            2 => DaemonResponse::RemoveOk,
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
                DaemonResponse::List(connections)
            }
            4 => DaemonResponse::ResetOk,
            255 => {
                let Some(msg) = decode_str(&mut cursor)? else {
                    return Ok(None);
                };
                DaemonResponse::Error(msg)
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unknown response type",
                ));
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
