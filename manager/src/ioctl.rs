use nix::errno::Errno;
use std::os::fd::AsRawFd;

#[repr(C)]
pub struct ConnectTo {
    pub addr: u32,
    pub port: u16,
}

mod raw {
    use super::*;
    use nix::ioctl_write_ptr;

    const TCPUART_IOC_MAGIC: u8 = b'T';
    const TCPUART_CONNECT_TO: u8 = 0;

    ioctl_write_ptr!(
        tcpuart_connect_to,
        TCPUART_IOC_MAGIC,
        TCPUART_CONNECT_TO,
        ConnectTo
    );
}

pub enum IoctlError {
    NoSlotsLeft,
    Other(Errno),
}

pub fn connect_to(file: &std::fs::File, mut to: ConnectTo) -> Result<i32, IoctlError> {
    to.addr = to.addr.to_be();
    to.port = to.port.to_be();
    match unsafe { raw::tcpuart_connect_to(file.as_raw_fd(), &to) } {
        Ok(minor) => Ok(minor),
        Err(Errno::ENOSPC) => Err(IoctlError::NoSlotsLeft),
        Err(err) => Err(IoctlError::Other(err)),
    }
}
