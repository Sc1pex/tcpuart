use std::io;
use bytes::{Buf, BufMut};

#[derive(Debug)]
pub enum CtlMessage {
    Add { name: String, addr: u32, port: u16 },
    Remove { name: String },
    List,
}

impl CtlMessage {
    pub fn msg_type(&self) -> u8 {
        match self {
            CtlMessage::Add { .. } => 1,
            CtlMessage::Remove { .. } => 2,
            CtlMessage::List => 3,
        }
    }

    pub fn encode(&self, dst: &mut impl BufMut) -> io::Result<()> {
        let name_size = match self {
            CtlMessage::Add { name, .. } | CtlMessage::Remove { name } => name.len(),
            CtlMessage::List => 0,
        };

        if name_size > 255 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "name too long"));
        }

        dst.put_u8(self.msg_type());

        match self {
            CtlMessage::Add { name, addr, port } => {
                dst.put_u8(name_size as u8);
                dst.put_slice(name.as_bytes());
                dst.put_u32(*addr);
                dst.put_u16(*port);
            }
            CtlMessage::Remove { name } => {
                dst.put_u8(name_size as u8);
                dst.put_slice(name.as_bytes());
            }
            CtlMessage::List => {}
        }
        Ok(())
    }

    pub fn decode(src: &mut impl Buf) -> io::Result<Option<Self>> {
        if src.remaining() < 1 {
            return Ok(None);
        }

        let message_kind = src.chunk()[0];

        match message_kind {
            1 => {
                if src.remaining() < 2 {
                    return Ok(None);
                }
                let name_len = src.chunk()[1] as usize;
                if src.remaining() < 2 + name_len + 4 + 2 {
                    return Ok(None);
                }
                src.advance(2);
                let mut name_vec = vec![0u8; name_len];
                src.copy_to_slice(&mut name_vec);
                let name = String::from_utf8(name_vec)
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid name"))?;
                let addr = src.get_u32();
                let port = src.get_u16();
                Ok(Some(CtlMessage::Add { name, addr, port }))
            }
            2 => {
                if src.remaining() < 2 {
                    return Ok(None);
                }
                let name_len = src.chunk()[1] as usize;
                if src.remaining() < 2 + name_len {
                    return Ok(None);
                }
                src.advance(2);
                let mut name_vec = vec![0u8; name_len];
                src.copy_to_slice(&mut name_vec);
                let name = String::from_utf8(name_vec)
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid name"))?;
                Ok(Some(CtlMessage::Remove { name }))
            }
            3 => {
                src.advance(1);
                Ok(Some(CtlMessage::List))
            }
            _ => Err(io::Error::new(io::ErrorKind::InvalidData, "unknown message type")),
        }
    }
}
