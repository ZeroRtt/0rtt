use std::{collections::VecDeque, io::ErrorKind, net::SocketAddr};

use metricrs::{Counter, global::get_global_registry};
use mio::{event::Source, net::UdpSocket};
use zerortt_api::WouldBlock;

use crate::buf::QuicBuf;

/// A `poll` error.
#[derive(Debug, thiserror::Error)]
pub(super) enum QuicSocketError {
    /// A quic protocol error.
    #[error("Send queue is full.")]
    IsFull(QuicBuf),

    #[error(transparent)]
    IO(#[from] std::io::Error),
}

impl From<QuicSocketError> for std::io::Error {
    fn from(value: QuicSocketError) -> Self {
        match value {
            QuicSocketError::IsFull(_) => std::io::Error::other(value),
            QuicSocketError::IO(error) => error,
        }
    }
}

type Result<T> = std::result::Result<T, QuicSocketError>;

/// Udp socket for quic protocol.
pub struct QuicSocket {
    sending_buffer_limits: usize,
    sending: VecDeque<(QuicBuf, SocketAddr)>,
    socket: UdpSocket,
    local_addr: SocketAddr,
    send_counter: Option<Counter>,
    recv_counter: Option<Counter>,
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
        let local_addr = socket.local_addr()?;

        let (send_counter, recv_counter) = if let Some(registry) = get_global_registry() {
            (
                Some(registry.counter(metricrs::Token::new(
                    "quic.socket.send",
                    &[("laddr", local_addr.to_string().as_str())],
                ))),
                Some(registry.counter(metricrs::Token::new(
                    "quic.socket.recv",
                    &[("laddr", local_addr.to_string().as_str())],
                ))),
            )
        } else {
            (None, None)
        };

        Ok(Self {
            local_addr,
            sending_buffer_limits,
            sending: Default::default(),
            socket,
            send_counter,
            recv_counter,
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
                .inspect(|send_size| {
                    if let Some(send_counter) = &self.send_counter {
                        send_counter.increment(*send_size as u64);
                    }
                })
                .inspect_err(|err| {
                    if err.kind() == ErrorKind::WouldBlock {
                        log::trace!("quic socket send data, pending");
                    } else {
                        log::error!("quic socket send data, target={}, err={}", target, err);
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
            _ = self.flush().map_err(std::io::Error::from).would_block()?;
        }

        if self.is_full() {
            return Err(QuicSocketError::IsFull(buf).into());
        }

        self.sending.push_back((buf, target));
        assert!(!(self.sending.len() > self.sending_buffer_limits));

        _ = self.flush().map_err(std::io::Error::from).would_block()?;

        Ok(self.sending.len())
    }

    /// Receive data from this socket.
    #[inline]
    pub fn recv_from(&self, buf: &mut QuicBuf) -> Result<SocketAddr> {
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

        if read_size > 0 {
            if let Some(counter) = &self.recv_counter {
                counter.increment(read_size as u64);
            }
        }

        log::trace!("quic socket recv data, len={}, from={}", read_size, from);

        buf.writable_consume(read_size);

        Ok(from)
    }
}
