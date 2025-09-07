//! Udp socket with send fifo queue.

use std::{collections::VecDeque, io::ErrorKind, net::SocketAddr};

use mio::{event::Source, net::UdpSocket};

use crate::{
    buf::QuicBuf,
    errors::{Error, Result},
    poll::WouldBlock,
};

/// Udp socket for quic protocol.
pub struct QuicSocket {
    sending_buffer_limits: usize,
    sending: VecDeque<(QuicBuf, SocketAddr)>,
    socket: UdpSocket,
    local_addr: SocketAddr,
}

impl Source for QuicSocket {
    #[inline]
    fn register(
        &mut self,
        registry: &mio::Registry,
        token: mio::Token,
        interests: mio::Interest,
    ) -> std::io::Result<()> {
        self.socket.register(registry, token, interests)
    }

    #[inline]
    fn reregister(
        &mut self,
        registry: &mio::Registry,
        token: mio::Token,
        interests: mio::Interest,
    ) -> std::io::Result<()> {
        self.socket.reregister(registry, token, interests)
    }

    #[inline]
    fn deregister(&mut self, registry: &mio::Registry) -> std::io::Result<()> {
        self.socket.deregister(registry)
    }
}

impl QuicSocket {
    /// Wrap a new `QuicSocket` from `mio::UdpSocket`.
    pub fn new(socket: UdpSocket, sending_buffer_limits: usize) -> Result<Self> {
        Ok(Self {
            local_addr: socket.local_addr()?,
            sending_buffer_limits,
            sending: Default::default(),
            socket,
        })
    }

    /// Returns the address bound to this socket.
    #[inline]
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Returns true if the sending buffer reach the `sending_buffer_limits`
    #[inline]
    pub fn is_full(&self) -> bool {
        self.sending.len() == self.sending_buffer_limits
    }

    /// Flush sending buffer.
    pub fn flush(&mut self) -> Result<()> {
        while !self.sending.is_empty() {
            let (buf, target) = self.sending.front().unwrap();

            self.socket
                .send_to(buf.readable_buf(), *target)
                .inspect_err(|err| {
                    if err.kind() == ErrorKind::WouldBlock {
                        log::trace!("quic socket send data, pending");
                    } else {
                        log::error!("quic socket send data, err={}", err);
                    }
                })?;

            log::trace!(
                "quic socket send data, len={}, target={}",
                buf.readable(),
                *target
            );

            self.sending.pop_front();
        }

        Ok(())
    }

    /// Sends data on the socket to the given address.
    ///
    /// Cache buffer into the sending queue, iff the underlying socket returns `WouldBlock` error.
    ///
    ///
    /// Returns pending buffer size, if success.
    pub fn send_to(&mut self, buf: QuicBuf, target: SocketAddr) -> Result<usize> {
        if self.is_full() {
            _ = self.flush().would_block()?;
        }

        if self.is_full() {
            return Err(Error::IsFull(buf));
        }

        self.sending.push_back((buf, target));
        assert!(!(self.sending.len() > self.sending_buffer_limits));

        _ = self.flush().would_block()?;

        Ok(self.sending.len())
    }

    /// Receive data from this socket.
    #[inline]
    pub fn recv_from(&mut self, buf: &mut QuicBuf) -> Result<SocketAddr> {
        let (read_size, from) = self
            .socket
            .recv_from(buf.writable_buf())
            .inspect_err(|err| {
                if err.kind() == ErrorKind::WouldBlock {
                    log::trace!("quic socket recv data, pending");
                } else {
                    log::error!("quic socket recv data, err={}", err);
                }
            })?;

        log::trace!("quic socket recv data, len={}, from={}", read_size, from);

        buf.writable_consume(read_size);

        Ok(from)
    }
}

#[cfg(test)]
mod tests {
    use std::{net::Ipv4Addr, thread::sleep, time::Duration};

    use mio::net::UdpSocket;

    use crate::{buf::QuicBuf, udp::QuicSocket};

    #[test]
    fn is_full() {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0).into()).unwrap();
        let mut socket = QuicSocket::new(socket, 1).unwrap();

        socket.sending.push_back((
            QuicBuf::from_slice(b"hello world").unwrap(),
            socket.local_addr(),
        ));

        assert_eq!(socket.is_full(), true);

        let port = socket.local_addr().port();

        assert_eq!(
            socket
                .send_to(
                    QuicBuf::from_slice(b"hello world").unwrap(),
                    (Ipv4Addr::LOCALHOST, port).into()
                )
                .unwrap(),
            0
        );

        sleep(Duration::from_secs(1));

        let mut buf = QuicBuf::new();

        let from = socket.recv_from(&mut buf).unwrap();

        assert_eq!(from.port(), socket.local_addr().port());

        assert_eq!(buf, QuicBuf::from_slice(b"hello world").unwrap());

        let mut buf = QuicBuf::new();

        let from = socket.recv_from(&mut buf).unwrap();

        assert_eq!(from.port(), socket.local_addr().port());

        assert_eq!(buf, QuicBuf::from_slice(b"hello world").unwrap());
    }

    #[test]
    fn send_to() {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0).into()).unwrap();

        let mut socket = QuicSocket::new(socket, 1024).unwrap();

        let port = socket.local_addr().port();

        assert_eq!(
            socket
                .send_to(
                    QuicBuf::from_slice(b"hello world").unwrap(),
                    (Ipv4Addr::LOCALHOST, port).into()
                )
                .unwrap(),
            0
        );

        assert_eq!(socket.is_full(), false);

        sleep(Duration::from_secs(1));

        let mut buf = QuicBuf::new();

        let from = socket.recv_from(&mut buf).unwrap();

        assert_eq!(from.port(), socket.local_addr().port());

        assert_eq!(buf, QuicBuf::from_slice(b"hello world").unwrap());
    }
}
