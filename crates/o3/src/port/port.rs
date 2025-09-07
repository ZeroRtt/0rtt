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
                if read_size == 0 {
                    return Err(Error::Fin(0, self.token()));
                }

                unsafe {
                    self.buf.writable_consume(read_size);
                }

                Ok(read_size)
            }
            Err(err) => Err(err),
        }
    }

    fn transfer_to(&mut self, other: &mut BufPort) -> Result<usize> {
        let mut total_send_size = 0;

        loop {
            while self.buf.readable() > 0 {
                match other.port.write(unsafe { self.buf.readable_buf() }) {
                    Ok(send_size) => {
                        // the underlying object is no longer able to accept bytes
                        if send_size == 0 {
                            log::trace!(
                                "transferred data, from={}, to={}, len={}, fin=true",
                                self.port.trace_id(),
                                other.port.trace_id(),
                                total_send_size
                            );

                            return Err(Error::Fin(total_send_size, other.token()));
                        }

                        total_send_size += send_size;
                        unsafe { self.buf.readable_consume(send_size) };
                    }
                    Err(Error::Retry) => {
                        if total_send_size > 0 {
                            log::trace!(
                                "transferred data, from={}, to={}, len={}",
                                self.port.trace_id(),
                                other.port.trace_id(),
                                total_send_size
                            );
                            return Ok(total_send_size);
                        } else {
                            log::trace!(
                                "transferred data, from={}, to={}, pending",
                                self.port.trace_id(),
                                other.port.trace_id(),
                            );
                            return Err(Error::Retry);
                        }
                    }
                    Err(err) => return Err(err),
                }
            }

            match self.read() {
                Ok(_) => {
                    assert!(self.buf.readable() > 0);
                }
                Err(Error::Fin(_, token)) => {
                    log::trace!(
                        "transferred data, from={}, to={}, len={}, fin=true",
                        self.port.trace_id(),
                        other.port.trace_id(),
                        total_send_size
                    );

                    return Err(Error::Fin(total_send_size, token));
                }
                Err(Error::Retry) => {
                    if total_send_size > 0 {
                        log::trace!(
                            "transferred data, from={}, to={}, len={}",
                            self.port.trace_id(),
                            other.port.trace_id(),
                            total_send_size
                        );
                        return Ok(total_send_size);
                    } else {
                        log::trace!(
                            "transferred data, from={}, to={}, pending",
                            self.port.trace_id(),
                            other.port.trace_id(),
                        );
                        return Err(Error::Retry);
                    }
                }
                Err(err) => return Err(err),
            }
        }
    }
}

/// Copy data from `source` to `sink`.
pub fn copy(source: &mut BufPort, sink: &mut BufPort) -> Result<usize> {
    source.transfer_to(sink)
}

#[cfg(test)]
mod tests {
    use std::cmp;

    use crate::{
        poll::WouldBlock,
        port::{BufPort, Port, copy},
    };

    struct MockReadRetry;

    impl Port for MockReadRetry {
        fn trace_id(&self) -> &str {
            "MockRetry"
        }

        fn token(&self) -> crate::token::Token {
            crate::token::Token::Mio(0)
        }

        fn write(&mut self, buf: &[u8]) -> crate::errors::Result<usize> {
            Ok(buf.len())
        }

        fn read(&mut self, _: &mut [u8]) -> crate::errors::Result<usize> {
            Err(crate::errors::Error::Retry)
        }

        fn close(&mut self) -> crate::errors::Result<()> {
            Ok(())
        }
    }

    struct MockFin;

    impl Port for MockFin {
        fn trace_id(&self) -> &str {
            "MockFin"
        }

        fn token(&self) -> crate::token::Token {
            crate::token::Token::Mio(0)
        }

        fn write(&mut self, _: &[u8]) -> crate::errors::Result<usize> {
            Ok(0)
        }

        fn read(&mut self, _: &mut [u8]) -> crate::errors::Result<usize> {
            Ok(0)
        }

        fn close(&mut self) -> crate::errors::Result<()> {
            Ok(())
        }
    }

    struct MockWriteRetry(usize, usize);

    impl Port for MockWriteRetry {
        fn trace_id(&self) -> &str {
            "MockRetry"
        }

        fn token(&self) -> crate::token::Token {
            crate::token::Token::Mio(0)
        }

        fn write(&mut self, _: &[u8]) -> crate::errors::Result<usize> {
            Err(crate::errors::Error::Retry)
        }

        fn read(&mut self, buf: &mut [u8]) -> crate::errors::Result<usize> {
            if self.0 >= self.1 {
                return Ok(0);
            }

            let len = cmp::min(self.1 - self.0, buf.len());

            self.0 += len;

            Ok(len)
        }

        fn close(&mut self) -> crate::errors::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_retry() {
        let mut from = BufPort::new(MockReadRetry, 11);

        unsafe {
            from.buf.writable_buf().copy_from_slice(b"01234567890");
            from.buf.writable_consume(11);
        }

        let mut to = BufPort::new(MockReadRetry, 11);

        assert_eq!(copy(&mut from, &mut to).ok(), Some(11));

        assert!(copy(&mut from, &mut to).would_block().is_pending());

        let mut from = BufPort::new(MockWriteRetry(0, 12), 11);

        assert_eq!(
            copy(&mut from, &mut to).expect_err("").is_fin(),
            Some((12, crate::token::Token::Mio(0)))
        );

        let mut from = BufPort::new(MockWriteRetry(0, 12), 11);
        let mut to = BufPort::new(MockWriteRetry(0, 12), 11);

        assert!(copy(&mut from, &mut to).would_block().is_pending());

        let mut from = BufPort::new(MockWriteRetry(0, 12), 11);
        let mut to = BufPort::new(MockFin, 11);

        assert_eq!(
            copy(&mut from, &mut to).expect_err("").is_fin(),
            Some((0, crate::token::Token::Mio(0)))
        );
    }
}
