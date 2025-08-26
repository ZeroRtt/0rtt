use std::{
    cell::UnsafeCell,
    collections::HashMap,
    io::{ErrorKind, Read, Write},
    rc::Rc,
};

use mio::net::TcpStream;
use quico::{Group, quiche};
use ringbuff::RingBuf;

struct BufferedPort {
    /// boxed port instance.
    port: Box<dyn Port>,
    /// Read buffer for this port.
    read_buffer: RingBuf,
}

impl BufferedPort {
    fn with_capacity<P>(port: P, len: usize) -> Self
    where
        P: Port + 'static,
    {
        Self {
            port: Box::new(port),
            read_buffer: RingBuf::with_capacity(len),
        }
    }

    fn read_to_buffer(&mut self) -> Result<usize, SwitchError> {
        if self.read_buffer.writable() == 0 {
            return Err(SwitchError::WouldBlock(self.port.token()));
        }

        match self.port.read(unsafe { self.read_buffer.writable_buf() }) {
            Ok(read_size) => {
                if read_size > 0 {
                    unsafe {
                        self.read_buffer.writable_consume(read_size);
                    }
                }

                log::trace!(
                    "read data from port, id={:?}, len={}",
                    self.port.trace_id(),
                    read_size
                );

                Ok(read_size)
            }
            Err(err) => Err(err),
        }
    }

    fn transfer_from(&mut self, other: &mut BufferedPort) -> Result<usize, SwitchError> {
        let mut send_size = 0;

        loop {
            match other.read_to_buffer() {
                Err(SwitchError::WouldBlock(_)) => {}
                Err(err) => return Err(err),
                _ => {}
            }

            // no data to send.
            if other.read_buffer.readable() == 0 {
                if send_size > 0 {
                    return Ok(send_size);
                }

                log::trace!(
                    "transfer data, from={}, to={}, source pending",
                    other.port.trace_id(),
                    self.port.trace_id(),
                );

                return Err(SwitchError::WouldBlock(other.port.token()));
            }

            while other.read_buffer.readable() > 0 {
                match self.port.write(unsafe { other.read_buffer.readable_buf() }) {
                    Ok(write_size) => {
                        if write_size > 0 {
                            unsafe {
                                other.read_buffer.readable_consume(write_size);
                            }
                        }

                        log::trace!(
                            "transfer data, from={}, to={}, len={}",
                            other.port.trace_id(),
                            self.port.trace_id(),
                            write_size
                        );

                        send_size += write_size;
                    }
                    Err(SwitchError::WouldBlock(token)) => {
                        if send_size > 0 {
                            return Ok(send_size);
                        } else {
                            log::trace!(
                                "transfer data, from={}, to={}, sink pending",
                                other.port.trace_id(),
                                self.port.trace_id(),
                            );
                            return Err(SwitchError::WouldBlock(token));
                        }
                    }
                    Err(err) => return Err(err),
                }
            }
        }
    }
}

/// Token reference to switch port.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub enum Token {
    Mio(usize),
    QuicStream(u32, u64),
}

impl From<mio::Token> for Token {
    fn from(value: mio::Token) -> Self {
        Self::Mio(value.0)
    }
}

impl From<(quico::Token, u64)> for Token {
    fn from(value: (quico::Token, u64)) -> Self {
        Self::QuicStream(value.0.0, value.1)
    }
}

/// Error used by switch mod.
#[derive(Debug, thiserror::Error)]
pub enum SwitchError {
    /// Unhandle io error.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// Unhandle quiche error.
    #[error(transparent)]
    Quiche(#[from] quiche::Error),

    /// Pipeline/channel is broken.
    #[error("port is shutdown, port={0:?}")]
    BrokenPipe(Token),

    /// The I/O operation is not completed. retry later.
    #[error("I/O Operation on the port({0:?}) would block, retry later.")]
    WouldBlock(Token),

    /// The port resource referenced by `token` is not found.
    #[error("Port resource is not found, {0:?}")]
    Port(Token),

    #[error("Unknown source for port({0:?}) send")]
    Source(Token),

    #[error("Unknown sink for port({0:?}) recv")]
    Sink(Token),
}

/// A port provides non-blocking I/O operations.
pub trait Port {
    /// Returns debug information.
    fn trace_id(&self) -> &str;

    /// Returns the token associates with this port.
    fn token(&self) -> Token;

    /// Write data over this port.
    fn write(&mut self, buf: &[u8]) -> Result<usize, SwitchError>;

    /// Read data from this port.
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, SwitchError>;

    /// Shuts down reading and writing from/to the port.
    fn close(&mut self) -> Result<(), SwitchError>;
}

/// In-memory data switch for O3 apps.
#[derive(Default)]
pub struct Switch {
    /// Registered ports table.
    ports: HashMap<Token, UnsafeCell<BufferedPort>>,
    /// Index of port's sinks.
    sinks: HashMap<Token, Token>,
    /// Index of port's sources.
    sources: HashMap<Token, Token>,
}

#[allow(unused)]
impl Switch {
    unsafe fn port(&self, token: &Token) -> Option<&'static mut BufferedPort> {
        self.ports
            .get(token)
            .map(|port| unsafe { port.get().as_mut().unwrap() })
    }
}

impl Switch {
    /// Register a new port with reading buffer size.
    pub fn register<P>(&mut self, port: P, buffer_size: usize) -> Result<(), SwitchError>
    where
        P: Port + 'static,
    {
        let token = port.token();

        let port = BufferedPort::with_capacity(port, buffer_size);

        assert!(
            self.ports.insert(token, UnsafeCell::new(port)).is_none(),
            "Register same port twice."
        );

        Ok(())
    }

    /// Deregister a port from this switch and drop buffered data.
    pub fn deregister<T>(&mut self, token: T) -> Result<Box<dyn Port>, SwitchError>
    where
        Token: From<T>,
    {
        let token = token.into();

        let port = self
            .ports
            .remove(&token)
            .ok_or_else(|| SwitchError::Port(token))?;

        // reset sink index.
        if let Some(sink) = self.sinks.remove(&token) {
            assert_eq!(self.sources.remove(&sink), Some(token));
        }

        // remove source index.
        if let Some(source) = self.sources.remove(&token) {
            assert_eq!(self.sinks.remove(&source), Some(token));
        }

        Ok(port.into_inner().port)
    }

    /// Send data over port.
    pub fn send<T>(&mut self, token: T) -> Result<usize, SwitchError>
    where
        Token: From<T>,
    {
        let sink = token.into();

        let Some(source) = self.sources.get(&sink).cloned() else {
            return Err(SwitchError::Source(sink));
        };

        let source = unsafe {
            self.port(&source)
                .ok_or_else(|| SwitchError::Port(source))?
        };

        let sink = unsafe { self.port(&sink).ok_or_else(|| SwitchError::Port(sink))? };

        sink.transfer_from(source)
    }

    /// Recv data from port.
    pub fn recv<T>(&mut self, token: T) -> Result<usize, SwitchError>
    where
        Token: From<T>,
    {
        let source = token.into();

        let Some(sink) = self.sinks.get(&source).cloned() else {
            return Err(SwitchError::Sink(source));
        };

        let source = unsafe {
            self.port(&source)
                .ok_or_else(|| SwitchError::Port(source))?
        };

        let sink = unsafe { self.port(&sink).ok_or_else(|| SwitchError::Port(sink))? };

        sink.transfer_from(source)
    }
}

/// A `Port` implementation backed with `TcpStream`.
pub struct TcpStreamPort {
    trace_id: String,
    socket: TcpStream,
    token: Token,
}

impl TcpStreamPort {
    pub fn new(token: mio::Token, socket: TcpStream) -> Result<Self, SwitchError> {
        let local_addr = socket.local_addr()?;
        let peer_addr = socket.peer_addr()?;

        let trace_id = format!("tcp({} => {})", local_addr, peer_addr);

        Ok(Self {
            trace_id,
            socket,
            token: token.into(),
        })
    }
}

impl Port for TcpStreamPort {
    fn trace_id(&self) -> &str {
        &self.trace_id
    }

    fn token(&self) -> Token {
        self.token
    }

    fn write(&mut self, buf: &[u8]) -> Result<usize, SwitchError> {
        match self.socket.write(buf) {
            Ok(write_size) => Ok(write_size),
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                Err(SwitchError::WouldBlock(self.token))
            }
            Err(err) => Err(SwitchError::Io(err)),
        }
    }

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, SwitchError> {
        match self.socket.read(buf) {
            Ok(write_size) => Ok(write_size),
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                Err(SwitchError::WouldBlock(self.token))
            }
            Err(err) => Err(SwitchError::Io(err)),
        }
    }

    fn close(&mut self) -> Result<(), SwitchError> {
        match self.socket.shutdown(std::net::Shutdown::Both) {
            Ok(write_size) => Ok(write_size),
            Err(err) => Err(SwitchError::Io(err)),
        }
    }
}

/// A `Port` implementation backed with `QuicStream`.
pub struct QuicStreamPort {
    trace_id: String,
    conn_id: quico::Token,
    stream_id: u64,
    group: Rc<Group>,
}

impl QuicStreamPort {
    pub fn new(group: Rc<Group>, conn_id: quico::Token, stream_id: u64) -> Self {
        Self {
            trace_id: format!("quic({},{})", conn_id.0, stream_id),
            group,
            conn_id,
            stream_id,
        }
    }
}

impl Port for QuicStreamPort {
    fn trace_id(&self) -> &str {
        &self.trace_id
    }

    fn token(&self) -> Token {
        Token::QuicStream(self.conn_id.0, self.stream_id)
    }

    fn write(&mut self, buf: &[u8]) -> Result<usize, SwitchError> {
        match self
            .group
            .stream_send(self.conn_id, self.stream_id, buf, false)
        {
            Ok(send_size) => Ok(send_size),
            Err(quico::Error::Busy) | Err(quico::Error::Retry) => {
                Err(SwitchError::WouldBlock(self.token()))
            }
            Err(quico::Error::Quiche(err)) => Err(SwitchError::Quiche(err)),
            Err(err) => unreachable!("quic(stream_send): unexpect error: {}", err),
        }
    }

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, SwitchError> {
        match self.group.stream_recv(self.conn_id, self.stream_id, buf) {
            Ok((recv_size, _)) => Ok(recv_size),
            Err(quico::Error::Busy) | Err(quico::Error::Retry) => {
                Err(SwitchError::WouldBlock(self.token()))
            }
            Err(quico::Error::Quiche(err)) => Err(SwitchError::Quiche(err)),
            Err(err) => unreachable!("quic(stream_recv): unexpect error: {}", err),
        }
    }

    fn close(&mut self) -> Result<(), SwitchError> {
        match self.group.stream_close(self.conn_id, self.stream_id) {
            Ok(_) => Ok(()),
            Err(quico::Error::Quiche(err)) => Err(SwitchError::Quiche(err)),
            Err(err) => unreachable!("quic(stream_close): unexpect error: {}", err),
        }
    }
}
