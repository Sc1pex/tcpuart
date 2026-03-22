use bytes::{Buf, BufMut, BytesMut};
use std::io;

pub fn encode_str(s: &str, dst: &mut BytesMut) -> io::Result<()> {
    let size = s.as_bytes().len();
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

pub fn decode_str(cursor: &mut io::Cursor<&mut BytesMut>) -> io::Result<Option<String>> {
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
