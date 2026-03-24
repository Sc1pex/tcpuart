use nix::{
    pty::PtyMaster,
    sys::{termios, uio::readv},
};
use std::{
    io::{self, IoSliceMut, Write},
    os::fd::AsRawFd,
    pin::Pin,
    task::{ready, Context, Poll},
};
use tokio::io::{unix::AsyncFd, AsyncWrite};

pub struct AsyncPty {
    inner: AsyncFd<PtyMaster>,
    current_tio: termios::Termios,
}

pub enum PtyReadResult {
    Data(usize),
    TermiosChange,
    ControlMessage(u8),
}

// Not defined in libc for linux for some reason, has the same value on all platforms
const TIOCPKT_IOCTL: u8 = 0x40;

nix::ioctl_write_ptr_bad!(tiocpkt, nix::libc::TIOCPKT, i32);

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

        // Required to get IOCTL packets
        let mut tio = termios::tcgetattr(&pty)?;
        tio.local_flags |= termios::LocalFlags::EXTPROC;
        termios::tcsetattr(&pty, termios::SetArg::TCSANOW, &tio)?;

        Ok(Self {
            inner: AsyncFd::new(pty)?,
            current_tio: tio,
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
                            let mut new_tio = termios::tcgetattr(&guard.get_inner())?;
                            if new_tio != self.current_tio {
                                if !new_tio.local_flags.contains(termios::LocalFlags::EXTPROC) {
                                    new_tio.local_flags |= termios::LocalFlags::EXTPROC;
                                    termios::tcsetattr(
                                        &guard.get_inner(),
                                        termios::SetArg::TCSANOW,
                                        &new_tio,
                                    )?;
                                    self.current_tio = new_tio;
                                }

                                return Ok(PtyReadResult::TermiosChange);
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
