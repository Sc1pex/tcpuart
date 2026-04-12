use std::io;
use tokio_util::bytes::{Buf, BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

pub const MAX_MESSAGE_LEN: usize = 255;

// Message is short-lived (created, sent, dropped immediately) and never stored
// in collections. Stack allocation is better than heap allocation for this usecase
#[allow(clippy::large_enum_variant)]
#[derive(Copy, Clone, Debug)]
pub enum Message {
    Data(u8, [u8; MAX_MESSAGE_LEN]),
    Config {
        baudrate: u32,
        data_bits: u8,
        stop_bits: u8,
        parity: u8,
    },
    ControlReq(MessageControlReq),
    ControlRes(MessageControlRes),
}

#[derive(Copy, Clone, Debug)]
#[repr(u8)]
pub enum MessageControlReq {
    Reset = 1,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageControlRes {
    Ok = 1,
    NotSupported = 2,
}

impl TryFrom<u8> for MessageControlReq {
    type Error = io::Error;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Reset),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Unknown control request",
            )),
        }
    }
}
impl TryFrom<u8> for MessageControlRes {
    type Error = io::Error;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Ok),
            2 => Ok(Self::NotSupported),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Unknown control response",
            )),
        }
    }
}

impl Message {
    fn kind(&self) -> u8 {
        match self {
            Message::Data(_, _) => 1,
            Message::Config { .. } => 2,
            Message::ControlReq(_) => 3,
            Message::ControlRes(_) => 4,
        }
    }

    fn data_size(&self) -> usize {
        match self {
            Message::Data(len, _) => *len as usize,
            Message::Config { .. } => 7,
            Message::ControlReq(_) => 1,
            Message::ControlRes(_) => 1,
        }
    }
}

impl From<&[u8]> for Message {
    /// Create a data message from a maximum of `MAX_MESSAGE_LEN` bytes
    fn from(buf: &[u8]) -> Self {
        let len = buf.len().min(MAX_MESSAGE_LEN);
        let mut arr = [0; MAX_MESSAGE_LEN];
        arr[..len].copy_from_slice(&buf[..len]);
        Self::Data(len as u8, arr)
    }
}

pub struct MessageCodec;

impl Encoder<Message> for MessageCodec {
    type Error = io::Error;

    fn encode(&mut self, item: Message, dst: &mut BytesMut) -> Result<(), Self::Error> {
        dst.reserve(2 + item.data_size());
        dst.put_u8(item.kind());
        dst.put_u8(item.data_size() as u8);
        match item {
            Message::Data(size, data) => {
                dst.put_slice(&data[..size as usize]);
                Ok(())
            }
            Message::Config {
                baudrate,
                data_bits,
                stop_bits,
                parity,
            } => {
                dst.put_u32(baudrate);
                dst.put_u8(data_bits);
                dst.put_u8(stop_bits);
                dst.put_u8(parity);
                Ok(())
            }
            Message::ControlReq(req) => {
                dst.put_u8(req as u8);
                Ok(())
            }
            Message::ControlRes(res) => {
                dst.put_u8(res as u8);
                Ok(())
            }
        }
    }
}

impl Decoder for MessageCodec {
    type Item = Message;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let mut cursor = io::Cursor::new(&mut *src);

        let Ok(kind) = cursor.try_get_u8() else {
            return Ok(None);
        };
        let Ok(data_len) = cursor.try_get_u8() else {
            return Ok(None);
        };

        if cursor.remaining() < data_len as usize {
            return Ok(None);
        }

        let item = match kind {
            1 => {
                let mut data = [0; MAX_MESSAGE_LEN];
                cursor.copy_to_slice(&mut data[..data_len as usize]);
                Message::Data(data_len, data)
            }
            2 => {
                let Ok(baudrate) = cursor.try_get_u32() else {
                    return Ok(None);
                };
                let Ok(data_bits) = cursor.try_get_u8() else {
                    return Ok(None);
                };
                let Ok(stop_bits) = cursor.try_get_u8() else {
                    return Ok(None);
                };
                let Ok(parity) = cursor.try_get_u8() else {
                    return Ok(None);
                };
                Message::Config {
                    baudrate,
                    data_bits,
                    stop_bits,
                    parity,
                }
            }
            3 => {
                let Ok(req) = cursor.try_get_u8().map(MessageControlReq::try_from) else {
                    return Ok(None);
                };
                Message::ControlReq(req?)
            }
            4 => {
                let Ok(res) = cursor.try_get_u8().map(MessageControlRes::try_from) else {
                    return Ok(None);
                };
                Message::ControlRes(res?)
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Unknown message kind",
                ));
            }
        };

        let bytes_read = cursor.position() as usize;
        src.advance(bytes_read);
        Ok(Some(item))
    }
}
