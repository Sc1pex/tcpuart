use bytes::{Buf, BufMut, BytesMut};
use std::io;

pub const MAX_MESSAGE_LEN: u8 = 255;

#[repr(u8)]
#[derive(Clone, Copy, Debug)]
pub enum MessageKind {
    Data = 0,
    Config = 1,
}

impl TryFrom<u8> for MessageKind {
    type Error = io::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(MessageKind::Data),
            1 => Ok(MessageKind::Config),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid message kind: {}", value),
            )),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MessageHeader {
    pub kind: MessageKind,
    pub size: u8,
}

impl MessageHeader {
    pub fn data(size: u8) -> Self {
        Self {
            kind: MessageKind::Data,
            size,
        }
    }
}

pub fn encode_message(header: MessageHeader, data: &[u8], dst: &mut BytesMut) -> io::Result<()> {
    if data.len() > MAX_MESSAGE_LEN as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Data length exceeds maximum of {}", MAX_MESSAGE_LEN),
        ));
    }

    // Header is 2 bytes
    dst.reserve(2 + data.len());

    dst.put_u8(header.kind as u8);
    dst.put_u8(header.size);
    dst.put_slice(data);

    Ok(())
}

pub fn decode_message(src: &mut BytesMut, data: &mut [u8]) -> io::Result<Option<MessageHeader>> {
    let mut cursor = io::Cursor::new(&mut *src);

    let kind = match cursor.try_get_u8().map(TryInto::try_into) {
        Ok(Ok(kind)) => kind,
        Ok(Err(e)) => return Err(e),
        Err(_) => return Ok(None),
    };
    let data_len = match cursor.try_get_u8() {
        Ok(kind) => kind,
        Err(_) => return Ok(None),
    };

    if cursor.remaining() < data_len as usize {
        return Ok(None);
    }

    cursor.copy_to_slice(&mut data[..data_len as usize]);
    let bytes_read = cursor.position() as usize;
    src.advance(bytes_read);
    Ok(Some(MessageHeader {
        kind,
        size: data_len,
    }))
}
