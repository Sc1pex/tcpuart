use std::io;
use tokio_util::bytes::{Buf, BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

pub const MAX_MESSAGE_LEN: usize = 255;

// Message is short-lived (created, sent, dropped immediately) and never stored
// in collections. Stack allocation is better than heap allocation for this usecase
#[allow(clippy::large_enum_variant)]
pub enum Message {
    Data(u8, [u8; MAX_MESSAGE_LEN]),
    Config {},
}

impl Message {
    fn kind(&self) -> u8 {
        match self {
            Message::Data(_, _) => 1,
            Message::Config {} => 2,
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

pub struct MessageEncoder;
pub struct MessageDecoder;

impl Encoder<Message> for MessageEncoder {
    type Error = io::Error;

    fn encode(&mut self, item: Message, dst: &mut BytesMut) -> Result<(), Self::Error> {
        match item {
            Message::Data(size, data) => {
                dst.reserve(2 + size as usize);
                dst.put_u8(item.kind());
                dst.put_u8(size);
                dst.put_slice(&data[..size as usize]);
                Ok(())
            }
            Message::Config {} => todo!(),
        }
    }
}

impl Decoder for MessageDecoder {
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

        let item: Message;
        match kind {
            1 => {
                let mut data = [0; MAX_MESSAGE_LEN];
                cursor.copy_to_slice(&mut data[..data_len as usize]);
                item = Message::Data(data_len, data);
            }
            2 => todo!(),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Unknown message kind",
                ))
            }
        }

        let bytes_read = cursor.position() as usize;
        src.advance(bytes_read);
        Ok(Some(item))
    }
}
