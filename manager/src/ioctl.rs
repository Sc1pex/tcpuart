use nix::errno::Errno;
use std::os::fd::AsRawFd;

#[repr(C)]
pub struct ConnectTo {
    pub addr: u32,
    pub port: u16,
}

#[repr(C)]
pub struct ServerInfo {
    pub minor: u32,
    pub addr: u32,
    pub port: u16,
}

mod raw {
    use super::*;
    use nix::{ioctl_readwrite, ioctl_write_int, ioctl_write_ptr};

    const TCPUART_IOC_MAGIC: u8 = b'T';
    const TCPUART_CONNECT_TO: u8 = 0;
    const TCPUART_DISCONNECT: u8 = 1;
    const TCPUART_GET_SERVER_INFO: u8 = 2;

    ioctl_write_ptr!(
        tcpuart_connect_to,
        TCPUART_IOC_MAGIC,
        TCPUART_CONNECT_TO,
        ConnectTo
    );

    ioctl_write_int!(tcpuart_disconnect, TCPUART_IOC_MAGIC, TCPUART_DISCONNECT);

    ioctl_readwrite!(
        tcpuart_get_server_info,
        TCPUART_IOC_MAGIC,
        TCPUART_GET_SERVER_INFO,
        ServerInfo
    );
}

pub enum IoctlError {
    NoSlotsLeft,
    DeviceNotFound,
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

pub fn disconnect(file: &std::fs::File, minor: u64) -> Result<(), IoctlError> {
    match unsafe { raw::tcpuart_disconnect(file.as_raw_fd(), minor) } {
        Ok(_) => Ok(()),
        Err(Errno::ENODEV) => Err(IoctlError::DeviceNotFound),
        Err(err) => Err(IoctlError::Other(err)),
    }
}

pub fn get_server_info(file: &std::fs::File, minor: u32) -> Result<ServerInfo, IoctlError> {
    let mut info = ServerInfo {
        minor,
        addr: 0,
        port: 0,
    };
    match unsafe { raw::tcpuart_get_server_info(file.as_raw_fd(), &mut info) } {
        Ok(_) => {
            info.addr = u32::from_be(info.addr);
            info.port = u16::from_be(info.port);
            Ok(info)
        }
        Err(Errno::ENODEV) => Err(IoctlError::DeviceNotFound),
        Err(err) => Err(IoctlError::Other(err)),
    }
}
