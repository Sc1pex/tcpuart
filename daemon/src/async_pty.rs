use nix::{
    fcntl,
    pty::PtyMaster,
    sys::{termios, uio::readv},
};
use std::{
    io::{self, IoSliceMut, Write},
    os::fd::{AsRawFd, OwnedFd},
    pin::Pin,
    task::{ready, Context, Poll},
};
use tokio::io::{unix::AsyncFd, AsyncWrite};

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

#[cfg(target_os = "macos")]
nix::ioctl_write_ptr_bad!(tiocextproc, nix::libc::TIOCEXTPROC, i32);

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
                    } else {
                        if ctrl[0] & TIOCPKT_IOCTL != 0 {
                            let mut new_tio = termios::tcgetattr(&self.slave_fd)?;
                            if new_tio != self.current_tio {
                                // Re-assert EXTPROC if it was cleared by the slave app
                                if !new_tio.local_flags.contains(termios::LocalFlags::EXTPROC) {
                                    set_extproc(self.inner.get_ref(), &self.slave_fd)?;
                                    // Refresh after setting
                                    new_tio = termios::tcgetattr(&self.slave_fd)?;
                                }

                                self.current_tio = new_tio;
                                return Ok(PtyReadResult::TermiosChange(get_termios_change(
                                    &self.current_tio,
                                )));
                            }
                        } else {
                            return Ok(PtyReadResult::ControlMessage(ctrl[0]));
                        }
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

#[allow(unused_variables)]
fn set_extproc(master: &PtyMaster, slave: &OwnedFd) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let mut tio = termios::tcgetattr(slave)?;
        tio.local_flags |= termios::LocalFlags::EXTPROC;
        termios::tcsetattr(slave, termios::SetArg::TCSANOW, &tio)?;
    }

    #[cfg(target_os = "macos")]
    {
        let on: i32 = 1;
        unsafe {
            tiocextproc(master.as_raw_fd(), &on)?;
        }
    }

    Ok(())
}

impl AsyncWrite for AsyncPty {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        loop {
            let mut guard = ready!(self.inner.poll_write_ready(cx))?;

            match guard.try_io(|inner| inner.get_ref().write(buf)) {
                Ok(result) => return Poll::Ready(result),
                Err(_would_block) => continue,
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

fn get_termios_change(tio: &termios::Termios) -> TermiosChange {
    let baudrate = termios::cfgetispeed(tio);
    let data_bits = match tio.control_flags & termios::ControlFlags::CSIZE {
        termios::ControlFlags::CS5 => 5,
        termios::ControlFlags::CS6 => 6,
        termios::ControlFlags::CS7 => 7,
        termios::ControlFlags::CS8 => 8,
        _ => 0,
    };
    let parity = if tio.control_flags.contains(termios::ControlFlags::PARENB)
        && !tio.control_flags.contains(termios::ControlFlags::PARODD)
    {
        1
    } else if tio.control_flags.contains(termios::ControlFlags::PARENB)
        && tio.control_flags.contains(termios::ControlFlags::PARODD)
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
        baudrate: baudrate as u32,
        data_bits,
        parity,
        stop_bits,
    }
}
