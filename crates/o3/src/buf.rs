//! Tiny and fixed length buffer for protocol parsing.

use std::{collections::VecDeque, fmt::Debug, io::ErrorKind, net::SocketAddr};

use mio::net::UdpSocket;

use crate::{
    errors::{Error, Result},
    would_block::WouldBlock,
};

/// Array backed buffer,with compile-time fixed capacity.
pub struct ArrayBuf<const LEN: usize> {
    /// fixed array buffer.
    buf: Box<[u8; LEN]>,
    /// written data length.
    len: usize,
}

impl<const LEN: usize> Debug for ArrayBuf<LEN> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ArrayBuf[{}]", LEN)
    }
}

impl<const LEN: usize> AsRef<[u8]> for ArrayBuf<LEN> {
    fn as_ref(&self) -> &[u8] {
        &self.buf.as_slice()[..self.len]
    }
}

impl<const LEN: usize> AsMut<[u8]> for ArrayBuf<LEN> {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.buf.as_mut_slice()[..self.len]
    }
}

impl<const LEN: usize> ArrayBuf<LEN> {
    /// Create a new ArrayBuf with default initialization data.
    pub fn new() -> Self {
        Self {
            buf: Box::new([0; LEN]),
            len: 0,
        }
    }
    /// Returns written data length.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns the capacity of this buffer.
    pub fn capacity(&self) -> usize {
        return LEN;
    }

    /// Returns the whole writable buffer slice.
    pub fn writable_buf(&mut self) -> &mut [u8] {
        self.buf.as_mut_slice()
    }

    /// Update written data length.
    pub fn truncate(&mut self, len: usize) {
        assert!(!(len > LEN), "Out of range, {}", len);
        self.len = len;
    }
}

/// Fixed-length buffer for quic protocol.
pub type QuicBuf = ArrayBuf<1330>;

/// Udp socket with sending fifo.
pub struct QuicSocket {
    max_pending_size: usize,
    sending: VecDeque<(QuicBuf, SocketAddr)>,
    socket: UdpSocket,
    local_addr: SocketAddr,
}

impl QuicSocket {
    /// Convert [`UdpSocket`] into quic socket(with sending fifo).
    pub fn new(socket: UdpSocket, local_addr: SocketAddr, max_pending_size: usize) -> Self {
        Self {
            local_addr,
            max_pending_size,
            sending: Default::default(),
            socket,
        }
    }

    /// Returns socket's local bound address.
    #[inline]
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Check if the sending `fifo` is full.
    #[inline]
    pub fn is_full(&self) -> bool {
        !(self.sending.len() < self.max_pending_size)
    }

    pub fn flush(&mut self) -> Result<()> {
        while !self.sending.is_empty() {
            let (buf, target) = self.sending.front().unwrap();

            log::trace!("buf={}, target={}", buf.as_ref().len(), *target);

            self.socket
                .send_to(buf.as_ref(), *target)
                .inspect_err(|err| {
                    if err.kind() == ErrorKind::WouldBlock {
                        log::trace!("UdpSocket send_to, pending");
                    } else {
                        log::error!("UdpSocket send_to, err={}", err);
                    }
                })?;

            self.sending.pop_front();
        }

        Ok(())
    }

    /// Send datagram over this socket.
    pub fn send_to(&mut self, buf: QuicBuf, target: SocketAddr) -> Result<usize> {
        _ = self.flush().would_block()?;

        if self.is_full() {
            return Err(Error::IsFull(buf));
        }

        let len = buf.len();
        self.sending.push_back((buf, target));

        _ = self.flush().would_block()?;

        Ok(len)
    }

    /// Receive data from this socket.
    pub fn recv_from(&mut self, buf: &mut QuicBuf) -> Result<SocketAddr> {
        let (read_size, from) = self.socket.recv_from(buf.writable_buf())?;

        buf.truncate(read_size);

        Ok(from)
    }
}

#[cfg(test)]
mod tests {

    use crate::buf::QuicBuf;

    #[test]
    fn test_array_buf() {
        let mut buf = QuicBuf::new();

        assert_eq!(buf.as_ref(), &[]);
        assert_eq!(buf.as_mut(), &mut []);

        assert_eq!(buf.len(), 0);
        assert_eq!(buf.capacity(), 1330);

        buf.truncate(1000);
        assert_eq!(buf.as_ref(), &[0; 1000]);
    }
}
