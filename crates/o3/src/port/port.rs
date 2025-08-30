//! Port is a non-blocking `I/O` stream for o3 system.

use ringbuff::RingBuf;

use crate::{
    errors::{Error, Result},
    token::Token,
};

/// A port is an abstraction of non-blocking `I/O` stream.
pub trait Port {
    /// Returns this port's debug label string.
    fn trace_id(&self) -> &str;

    /// Returns port token.
    fn token(&self) -> Token;

    /// Write data over this port.
    fn write(&mut self, buf: &[u8]) -> Result<usize>;

    /// Read data from this port.
    fn read(&mut self, buf: &mut [u8]) -> Result<usize>;

    /// Close this port.
    fn close(&mut self) -> Result<()>;
}

/// A port with reading buffer.
pub struct BufPort {
    port: Box<dyn Port>,
    buf: RingBuf,
}

impl BufPort {
    /// Create a new `buffered` port.
    pub fn new<P: Port + 'static>(port: P, buf_size: usize) -> Self {
        Self {
            port: Box::new(port),
            buf: RingBuf::with_capacity(buf_size),
        }
    }

    /// Returns this port's debug label string.
    #[inline]
    pub fn trace_id(&self) -> &str {
        self.port.trace_id()
    }

    /// Returns port token.
    #[inline]
    pub fn token(&self) -> Token {
        self.port.token()
    }

    /// Close this port.
    #[inline]
    pub fn close(&mut self) -> Result<()> {
        self.port.close()
    }

    fn read(&mut self) -> Result<usize> {
        if self.buf.writable() == 0 {
            return Err(Error::Retry);
        }

        match self.port.read(unsafe { self.buf.writable_buf() }) {
            Ok(read_size) => {
                if read_size > 0 {
                    unsafe {
                        self.buf.writable_consume(read_size);
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

    fn transfer_from(&mut self, other: &mut BufPort) -> Result<usize> {
        let mut send_size = 0;

        loop {
            match other.read() {
                Err(Error::Retry) => {}
                Err(err) => return Err(err),
                _ => {}
            }

            // no data to send.
            if other.buf.readable() == 0 {
                if send_size > 0 {
                    return Ok(send_size);
                }

                log::trace!(
                    "transfer data, from={}, to={}, source pending",
                    other.port.trace_id(),
                    self.port.trace_id(),
                );

                return Err(Error::Retry);
            }

            while other.buf.readable() > 0 {
                match self.port.write(unsafe { other.buf.readable_buf() }) {
                    Ok(write_size) => {
                        if write_size > 0 {
                            unsafe {
                                other.buf.readable_consume(write_size);
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
                    Err(Error::Retry) => {
                        if send_size > 0 {
                            return Ok(send_size);
                        } else {
                            log::trace!(
                                "transfer data, from={}, to={}, sink pending",
                                other.port.trace_id(),
                                self.port.trace_id(),
                            );
                            return Err(Error::Retry);
                        }
                    }
                    Err(err) => return Err(err),
                }
            }
        }
    }
}

/// Copy data from `source` to `sink`.
pub fn copy(source: &mut BufPort, sink: &mut BufPort) -> Result<usize> {
    sink.transfer_from(source)
}
