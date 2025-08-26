use std::{
    collections::VecDeque,
    io::{Error, ErrorKind},
    net::SocketAddr,
};

use mio::net::UdpSocket;

use crate::switch::SwitchError;

pub struct QuicBuf<const LEN: usize> {
    buf: Box<[u8; LEN]>,
    read_len: usize,
}

impl<const LEN: usize> AsRef<[u8]> for QuicBuf<LEN> {
    fn as_ref(&self) -> &[u8] {
        &self.buf.as_slice()[..self.read_len]
    }
}

impl<const LEN: usize> AsMut<[u8]> for QuicBuf<LEN> {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.buf.as_mut_slice()[..self.read_len]
    }
}

impl<const LEN: usize> QuicBuf<LEN> {
    pub fn new() -> Self {
        Self {
            // TODO: maybe uninit.
            buf: Box::new([0; LEN]),
            read_len: 0,
        }
    }

    pub fn writable_buf(&mut self) -> &mut [u8] {
        self.buf.as_mut_slice()
    }

    pub fn written_bytes(&mut self, len: usize) {
        assert!(!(len > LEN), "written_bytes: invalid input.");
        self.read_len = len;
    }
}

pub struct BufferedDatagram<Buf> {
    local_addr: SocketAddr,
    max_buffered_size: usize,
    send_datagrams: VecDeque<(Buf, SocketAddr)>,
    socket: UdpSocket,
}

impl<Buf> BufferedDatagram<Buf> {
    /// Create a new buffered datagram socket.
    pub fn new(socket: UdpSocket, max_buffered_size: usize) -> Self {
        Self {
            local_addr: socket.local_addr().unwrap(),
            send_datagrams: Default::default(),
            socket,
            max_buffered_size,
        }
    }
}

impl<Buf> BufferedDatagram<Buf>
where
    Buf: AsRef<[u8]>,
{
    pub fn is_full(&self) -> bool {
        self.send_datagrams.len() == self.max_buffered_size
    }

    #[inline]
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// flush buffered datagrams.
    pub fn flush(&mut self) -> std::io::Result<()> {
        while !self.send_datagrams.is_empty() {
            let (buf, target) = self.send_datagrams.front().unwrap();

            match self.socket.send_to(buf.as_ref(), *target) {
                Ok(send_size) => {
                    log::trace!("send datagram to {}, len={}", target, send_size);
                    self.send_datagrams.pop_front();
                    continue;
                }
                Err(err) => return Err(err),
            }
        }

        Ok(())
    }

    pub fn send_to(&mut self, buf: Buf, target: SocketAddr) -> std::io::Result<()> {
        _ = self.flush().would_block()?;

        match self.socket.send_to(buf.as_ref(), target) {
            Ok(send_size) => {
                log::trace!("send datagram to {}, len={}", target, send_size);
                Ok(())
            }
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                if self.send_datagrams.len() == self.max_buffered_size {
                    return Err(Error::new(
                        ErrorKind::OutOfMemory,
                        "BufferedDatagram: overflow.",
                    ));
                }
                self.send_datagrams.push_back((buf, target));
                Ok(())
            }
            Err(_) => todo!(),
        }
    }

    #[inline]
    pub fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
        self.socket.recv_from(buf)
    }
}

/// Convert an result to `std::task::Poll`
pub trait WouldBlock<T> {
    type Error;

    fn would_block(self) -> std::task::Poll<Result<T, Self::Error>>;
}

impl<T> WouldBlock<T> for Result<T, std::io::Error> {
    type Error = std::io::Error;

    fn would_block(self) -> std::task::Poll<Result<T, Self::Error>> {
        match self {
            Err(err) if err.kind() == ErrorKind::WouldBlock => std::task::Poll::Pending,
            r => std::task::Poll::Ready(r),
        }
    }
}

impl<T> WouldBlock<T> for Result<T, quico::Error> {
    type Error = quico::Error;

    fn would_block(self) -> std::task::Poll<Result<T, Self::Error>> {
        match self {
            Err(quico::Error::Busy) | Err(quico::Error::Retry) => std::task::Poll::Pending,
            r => std::task::Poll::Ready(r),
        }
    }
}

impl<T> WouldBlock<T> for Result<T, SwitchError> {
    type Error = SwitchError;

    fn would_block(self) -> std::task::Poll<Result<T, Self::Error>> {
        match self {
            Err(SwitchError::WouldBlock(_)) => std::task::Poll::Pending,
            r => std::task::Poll::Ready(r),
        }
    }
}
