use std::io;
use tokio_util::bytes::{Buf, BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};
use tracing::{error, trace};

pub const MAX_MESSAGE_LEN: usize = 255;

/// Represents the physical protocol messages exchanged between the Daemon and the ESP32 device
#[allow(clippy::large_enum_variant)]
#[derive(Copy, Clone, Debug)]
pub enum DeviceMessage {
    Data(u8, [u8; MAX_MESSAGE_LEN]),
    Config {
        baudrate: u32,
        data_bits: u8,
        stop_bits: u8,
        parity: u8,
    },
    ControlReq(DeviceControlRequest),
    ControlRes(DeviceControlResponse),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DeviceControlRequest {
    Reset = 1,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DeviceControlResponse {
    Ok = 1,
    NotSupported = 2,
}

impl TryFrom<u8> for DeviceControlRequest {
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
impl TryFrom<u8> for DeviceControlResponse {
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

impl DeviceMessage {
    fn kind(&self) -> u8 {
        match self {
            DeviceMessage::Data(_, _) => 1,
            DeviceMessage::Config { .. } => 2,
            DeviceMessage::ControlReq(_) => 3,
            DeviceMessage::ControlRes(_) => 4,
        }
    }

    fn data_size(&self) -> usize {
        match self {
            DeviceMessage::Data(len, _) => *len as usize,
            DeviceMessage::Config { .. } => 7,
            DeviceMessage::ControlReq(_) => 1,
            DeviceMessage::ControlRes(_) => 1,
        }
    }
}

impl From<&[u8]> for DeviceMessage {
    /// Create a data message from a maximum of `MAX_MESSAGE_LEN` bytes
    fn from(buf: &[u8]) -> Self {
        let len = buf.len().min(MAX_MESSAGE_LEN);
        let mut arr = [0; MAX_MESSAGE_LEN];
        arr[..len].copy_from_slice(&buf[..len]);
        Self::Data(len as u8, arr)
    }
}

pub struct DeviceCodec;

impl Encoder<DeviceMessage> for DeviceCodec {
    type Error = io::Error;

    fn encode(&mut self, item: DeviceMessage, dst: &mut BytesMut) -> Result<(), Self::Error> {
        dst.reserve(2 + item.data_size());
        dst.put_u8(item.kind());
        dst.put_u8(item.data_size() as u8);
        match item {
            DeviceMessage::Data(size, data) => {
                dst.put_slice(&data[..size as usize]);
                Ok(())
            }
            DeviceMessage::Config {
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
            DeviceMessage::ControlReq(req) => {
                dst.put_u8(req as u8);
                Ok(())
            }
            DeviceMessage::ControlRes(res) => {
                dst.put_u8(res as u8);
                Ok(())
            }
        }
    }
}

impl Decoder for DeviceCodec {
    type Item = DeviceMessage;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let mut cursor = io::Cursor::new(&mut *src);

        let Ok(kind) = cursor.try_get_u8() else {
            return Ok(None);
        };
        let Ok(data_len) = cursor.try_get_u8() else {
            return Ok(None);
        };

        trace!(kind, data_len, "found message header");

        if cursor.remaining() < data_len as usize {
            return Ok(None);
        }

        let item = match kind {
            1 => {
                let mut data = [0; MAX_MESSAGE_LEN];
                cursor.copy_to_slice(&mut data[..data_len as usize]);
                DeviceMessage::Data(data_len, data)
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
                DeviceMessage::Config {
                    baudrate,
                    data_bits,
                    stop_bits,
                    parity,
                }
            }
            3 => {
                let Ok(req) = cursor.try_get_u8().map(DeviceControlRequest::try_from) else {
                    return Ok(None);
                };
                DeviceMessage::ControlReq(req?)
            }
            4 => {
                let Ok(res) = cursor.try_get_u8().map(DeviceControlResponse::try_from) else {
                    return Ok(None);
                };
                DeviceMessage::ControlRes(res?)
            }
            _ => {
                error!(kind, "received unknown message kind");
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Unknown message kind",
                ));
            }
        };

        let bytes_read = cursor.position() as usize;
        src.advance(bytes_read);
        trace!(?item, "successfully decoded message");
        Ok(Some(item))
    }
}
