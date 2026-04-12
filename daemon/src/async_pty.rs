use nix::{
    fcntl,
    pty::PtyMaster,
    sys::{termios, uio::readv},
};
use std::{
    io::{self, IoSliceMut, Write},
    os::fd::{AsRawFd, OwnedFd},
    pin::Pin,
    task::{Context, Poll, ready},
};
use tokio::io::{AsyncWrite, unix::AsyncFd};

pub struct AsyncPty {
    inner: AsyncFd<PtyMaster>,
    current_tio: termios::Termios,

    slave_fd: OwnedFd,
}

pub struct TermiosChange {
    pub baudrate: u32,
    pub data_bits: u8,
    pub parity: u8,
    pub stop_bits: u8,
}

pub enum PtyReadResult {
    Data(usize),
    TermiosChange(TermiosChange),
    ControlMessage(u8),
}

// Not defined in libc for linux for some reason, has the same value on all platforms
const TIOCPKT_IOCTL: u8 = 0x40;

nix::ioctl_write_ptr_bad!(tiocpkt, nix::libc::TIOCPKT, i32);

#[cfg(not(target_os = "linux"))]
nix::ioctl_write_ptr_bad!(tiocextproc, nix::libc::TIOCEXT, i32);

impl AsyncPty {
    pub fn new(pty: PtyMaster) -> io::Result<Self> {
        let flags = nix::fcntl::fcntl(&pty, nix::fcntl::FcntlArg::F_GETFL)?;
        nix::fcntl::fcntl(
            &pty,
            nix::fcntl::FcntlArg::F_SETFL(
                nix::fcntl::OFlag::from_bits_truncate(flags) | nix::fcntl::OFlag::O_NONBLOCK,
            ),
        )?;

        // Enable packet mode
        unsafe {
            tiocpkt(pty.as_raw_fd(), &1i32)?;
        }

        let slave_name = unsafe { nix::pty::ptsname(&pty)? };
        let slave_fd = fcntl::open(
            slave_name.as_str(),
            fcntl::OFlag::O_RDWR | fcntl::OFlag::O_NOCTTY,
            nix::sys::stat::Mode::empty(),
        )?;

        // Required to get IOCTL packets
        set_extproc(&pty, &slave_fd)?;

        let tio = termios::tcgetattr(&slave_fd)?;

        Ok(Self {
            inner: AsyncFd::new(pty)?,
            current_tio: tio,
            slave_fd,
        })
    }

    pub async fn read(&mut self, buf: &mut [u8]) -> io::Result<PtyReadResult> {
        let mut ctrl = [0u8; 1];
        loop {
            let mut guard = self.inner.readable().await?;

            match readv(
                guard.get_inner(),
                &mut [IoSliceMut::new(&mut ctrl), IoSliceMut::new(buf)],
            ) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "True EOF reached",
                    ));
                }
                Ok(n) => {
                    if ctrl[0] == 0 {
                        if n > 1 {
                            return Ok(PtyReadResult::Data(n - 1));
                        }
                    } else if ctrl[0] & TIOCPKT_IOCTL != 0 {
                        let mut new_tio = termios::tcgetattr(&self.slave_fd)?;

                        // Re-assert EXTPROC if it was cleared by the slave app
                        if !new_tio.local_flags.contains(termios::LocalFlags::EXTPROC) {
                            set_extproc(self.inner.get_ref(), &self.slave_fd)?;
                            // Refresh after setting
                            new_tio = termios::tcgetattr(&self.slave_fd)?;
                        }

                        if check_termios_change(&self.current_tio, &new_tio) {
                            self.current_tio = new_tio;
                            return Ok(PtyReadResult::TermiosChange(get_termios_change(
                                &self.current_tio,
                            )));
                        }
                    } else {
                        return Ok(PtyReadResult::ControlMessage(ctrl[0]));
                    }
                }
                Err(nix::errno::Errno::EAGAIN) => {
                    guard.clear_ready();
                }
                Err(e) => {
                    return Err(e.into());
                }
            }
        }
    }
}

impl AsyncWrite for AsyncPty {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        loop {
            let mut guard = ready!(self.inner.poll_write_ready(cx))?;

            if let Ok(result) = guard.try_io(|inner| inner.get_ref().write(buf)) {
                return Poll::Ready(result);
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

fn speed_to_u32(speed: termios::BaudRate) -> u32 {
    #[cfg(target_os = "linux")]
    {
        use termios::BaudRate::*;
        match speed {
            B50 => 50,
            B75 => 75,
            B110 => 110,
            B134 => 134,
            B150 => 150,
            B200 => 200,
            B300 => 300,
            B600 => 600,
            B1200 => 1200,
            B1800 => 1800,
            B2400 => 2400,
            B4800 => 4800,
            B9600 => 9600,
            B19200 => 19200,
            B38400 => 38400,
            B57600 => 57600,
            B115200 => 115200,
            B230400 => 230400,
            B460800 => 460800,
            B500000 => 500000,
            B576000 => 576000,
            B921600 => 921600,
            B1000000 => 1000000,
            B1152000 => 1152000,
            B1500000 => 1500000,
            B2000000 => 2000000,
            B2500000 => 2500000,
            B3000000 => 3000000,
            B3500000 => 3500000,
            B4000000 => 4000000,
            _ => 9600,
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        speed as u32
    }
}

fn get_termios_change(tio: &termios::Termios) -> TermiosChange {
    let baudrate = speed_to_u32(termios::cfgetispeed(tio));
    let data_bits = match tio.control_flags & termios::ControlFlags::CSIZE {
        termios::ControlFlags::CS5 => 5,
        termios::ControlFlags::CS6 => 6,
        termios::ControlFlags::CS7 => 7,
        termios::ControlFlags::CS8 => 8,
        _ => 0,
    };
    let parity = if tio.control_flags.contains(termios::ControlFlags::PARENB)
        && tio.control_flags.contains(termios::ControlFlags::PARODD)
    {
        1
    } else if tio.control_flags.contains(termios::ControlFlags::PARENB)
        && !tio.control_flags.contains(termios::ControlFlags::PARODD)
    {
        2
    } else {
        0
    };
    let stop_bits = if tio.control_flags.contains(termios::ControlFlags::CSTOPB) {
        2
    } else {
        1
    };

    TermiosChange {
        baudrate,
        data_bits,
        parity,
        stop_bits,
    }
}

#[allow(unused_variables)]
fn set_extproc(master: &PtyMaster, slave: &OwnedFd) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let mut tio = termios::tcgetattr(slave)?;
        tio.local_flags |= termios::LocalFlags::EXTPROC;
        termios::tcsetattr(slave, termios::SetArg::TCSANOW, &tio)?;
    }

    #[cfg(not(target_os = "linux"))]
    {
        let on: i32 = 1;
        unsafe {
            tiocextproc(master.as_raw_fd(), &on)?;
        }
    }

    Ok(())
}

fn check_termios_change(old: &termios::Termios, new: &termios::Termios) -> bool {
    // Check if baudrate, data bits, parity or stop bits changed
    let old_baud = termios::cfgetispeed(old);
    let new_baud = termios::cfgetispeed(new);
    if old_baud != new_baud {
        return true;
    }

    let old_data_bits = old.control_flags & termios::ControlFlags::CSIZE;
    let new_data_bits = new.control_flags & termios::ControlFlags::CSIZE;
    if old_data_bits != new_data_bits {
        return true;
    }

    let old_parity =
        old.control_flags & (termios::ControlFlags::PARENB | termios::ControlFlags::PARODD);
    let new_parity =
        new.control_flags & (termios::ControlFlags::PARENB | termios::ControlFlags::PARODD);
    if old_parity != new_parity {
        return true;
    }

    let old_stop_bits = old.control_flags & termios::ControlFlags::CSTOPB;
    let new_stop_bits = new.control_flags & termios::ControlFlags::CSTOPB;
    return old_stop_bits != new_stop_bits;
}
