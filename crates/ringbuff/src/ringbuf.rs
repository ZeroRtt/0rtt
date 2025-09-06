//! RingBuf implementation for synchronous/asynchronous programming.

use std::{
    cmp,
    fmt::Debug,
    io::Result,
    ops::{Range, RangeTo},
    task::Poll,
};

use futures::io::{AsyncBufRead, AsyncRead, AsyncWrite};

// An in-memory ring buffer implemenation.
pub struct RingBuf {
    /// inner memory block.
    memory_block: Vec<u8>,
    /// cursor for read position.
    read_pos: u64,
    /// cursor for write position.
    write_pos: u64,
}

impl Debug for RingBuf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RingBuf")
            .field("memory", &self.memory_block.len())
            .field("read", &self.read_pos)
            .field("write", &self.write_pos)
            .finish()
    }
}

impl RingBuf {
    /// Create a ringbuf with specify capacity.
    pub fn with_capacity(len: usize) -> Self {
        assert!(len > 0, "capacity is zero.");
        Self {
            memory_block: vec![0; len],
            read_pos: 0,
            write_pos: 0,
        }
    }

    /// Returns the length of readable data.
    pub fn readable(&self) -> usize {
        (self.write_pos - self.read_pos) as usize
    }

    /// Returns the capacity of writable data.
    pub fn writable(&self) -> usize {
        (self.read_pos + self.memory_block.len() as u64 - self.write_pos) as usize
    }

    /// Read data from the ringbuf.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let read_size = cmp::min(self.readable(), buf.len());

        if read_size == 0 {
            return Ok(0);
        }

        match self.readable_ranges(read_size) {
            (first, Some(second)) => {
                let first_part = &self.memory_block[first];
                let second_part = &self.memory_block[second];
                buf[..first_part.len()].copy_from_slice(first_part);

                buf[first_part.len()..first_part.len() + second_part.len()]
                    .copy_from_slice(second_part);
            }
            (first, None) => {
                let first_part = &self.memory_block[first];
                buf[..first_part.len()].copy_from_slice(first_part);
            }
        }

        self.read_pos += read_size as u64;

        Ok(read_size)
    }

    /// Write data into ringbuf.
    pub fn write(&mut self, buf: &[u8]) -> Result<usize> {
        let write_size = cmp::min(self.writable(), buf.len());

        if write_size == 0 {
            return Ok(0);
        }

        match self.writable_ranges(write_size) {
            (first, Some(second)) => {
                let first_source = &buf[..first.len()];
                self.memory_block[first].copy_from_slice(first_source);

                let second_part = &mut self.memory_block[second];
                second_part.copy_from_slice(
                    &buf[first_source.len()..first_source.len() + second_part.len()],
                );
            }
            (first, None) => {
                let first_source = &buf[..first.len()];
                self.memory_block[first].copy_from_slice(first_source);
            }
        }

        self.write_pos += write_size as u64;

        Ok(write_size)
    }

    /// Unsafe function, directly returns first part of writable memory blocks.
    pub unsafe fn writable_buf(&mut self) -> &mut [u8] {
        let (range, _) = self.writable_ranges(self.writable());
        &mut self.memory_block[range]
    }

    /// Unsafe function, directly returns first part of redable memory blocks.
    pub unsafe fn readable_buf(&mut self) -> &[u8] {
        let (range, _) = self.readable_ranges(self.readable());
        &self.memory_block[range]
    }

    /// Unsafe function, directly advance readable cursor .
    pub unsafe fn readable_consume(&mut self, amt: usize) {
        self.read_pos += amt as u64;
        assert!(
            !(self.read_pos > self.write_pos),
            "advance_readable_pos: overflow"
        );
    }

    /// Unsafe function, directly advance writable cursor .
    pub unsafe fn writable_consume(&mut self, amt: usize) {
        self.write_pos += amt as u64;
        assert!(
            !((self.read_pos + self.memory_block.len() as u64) < self.write_pos),
            "advance_writable_pos: overflow"
        );
    }

    fn writable_ranges(&self, write_size: usize) -> (Range<usize>, Option<RangeTo<usize>>) {
        let write_end_pos = self.write_pos + write_size as u64;

        assert!(
            !(write_end_pos > self.read_pos + self.memory_block.len() as u64),
            "write_size is overflow."
        );

        let start = (self.write_pos % self.memory_block.len() as u64) as usize;
        let end = (write_end_pos % self.memory_block.len() as u64) as usize;

        if write_size == 0 {
            return (start..end, None);
        }

        if start < end {
            (start..end, None)
        } else {
            (start..self.memory_block.len(), Some(..end))
        }
    }

    fn readable_ranges(&self, read_size: usize) -> (Range<usize>, Option<RangeTo<usize>>) {
        let read_end_pos = self.read_pos + read_size as u64;

        assert!(!(read_end_pos > self.write_pos), "read_size is overflow.");

        let start = (self.read_pos % self.memory_block.len() as u64) as usize;
        let end = (read_end_pos % self.memory_block.len() as u64) as usize;

        if read_size == 0 {
            return (start..end, None);
        }

        if start < end {
            (start..end, None)
        } else {
            (start..self.memory_block.len(), Some(..end))
        }
    }
}

impl std::io::Read for RingBuf {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        Self::read(self, buf)
    }
}

impl std::io::Write for RingBuf {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        Self::write(self, buf)
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(feature = "async")]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
impl AsyncRead for RingBuf {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<Result<usize>> {
        Poll::Ready(self.read(buf))
    }
}

#[cfg(feature = "async")]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
impl AsyncBufRead for RingBuf {
    fn poll_fill_buf(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<&[u8]>> {
        unsafe { Poll::Ready(Ok(self.get_mut().readable_buf())) }
    }

    fn consume(self: std::pin::Pin<&mut Self>, amt: usize) {
        unsafe {
            self.get_mut().readable_consume(amt);
        }
    }
}

#[cfg(feature = "async")]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
impl AsyncWrite for RingBuf {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize>> {
        Poll::Ready(self.write(buf))
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[cfg(not(target_os = "windows"))]
    #[test_fuzz::test_fuzz]
    fn fuzz_test_unsafe_fn(offset: usize) {
        let offset = offset % 11;
        let mut ringbuf = RingBuf::with_capacity(11);

        unsafe {
            assert_eq!(ringbuf.writable_buf().len(), 11);
            ringbuf.writable_consume(offset);

            assert_eq!(ringbuf.writable_buf().len(), 11 - offset);

            assert_eq!(ringbuf.readable_buf().len(), offset);
        }
    }

    #[test]
    fn test_unsafe_fns() {
        let mut ringbuf = RingBuf::with_capacity(11);
        unsafe {
            assert_eq!(ringbuf.writable_buf().len(), 11);
            ringbuf.writable_buf().copy_from_slice(b"12345678901");
            ringbuf.writable_consume(5);
            assert_eq!(ringbuf.writable_buf().len(), 6);
            assert_eq!(ringbuf.readable_buf().len(), 5);
            ringbuf.writable_consume(6);
            assert_eq!(ringbuf.writable(), 0);
            assert_eq!(ringbuf.writable_buf().len(), 0);
            assert_eq!(ringbuf.readable_buf().len(), 11);
            ringbuf.readable_consume(3);
            assert_eq!(ringbuf.readable_buf(), b"45678901");
            assert_eq!(ringbuf.writable_buf(), b"123");
        }
    }

    #[test]
    fn test_pos() {
        let mut ringbuf = RingBuf::with_capacity(11);

        unsafe {
            ringbuf.writable_consume(11);
            assert_eq!(ringbuf.readable(), 11);
            assert_eq!(ringbuf.writable(), 0);
            ringbuf.readable_consume(11);
            assert_eq!(ringbuf.readable(), 0);
            assert_eq!(ringbuf.writable(), 11);
            ringbuf.writable_consume(10);
            ringbuf.readable_consume(9);
            assert_eq!(ringbuf.readable(), 1);
            assert_eq!(ringbuf.writable(), 10);
        }
    }

    #[test]
    fn test_io() {
        let mut ringbuf = RingBuf::with_capacity(11);

        assert_eq!(ringbuf.write(b"12345678901234").unwrap(), 11);
        assert_eq!(ringbuf.writable(), 0);
        assert_eq!(ringbuf.readable(), 11);

        let mut buf = vec![0; 12];

        assert_eq!(ringbuf.read(&mut buf).unwrap(), 11);
        assert_eq!(&buf[..11], b"12345678901");
        assert_eq!(ringbuf.writable(), 11);
        assert_eq!(ringbuf.readable(), 0);

        unsafe {
            ringbuf.writable_consume(5);
            ringbuf.readable_consume(5);
            assert_eq!(ringbuf.writable(), 11);
            assert_eq!(ringbuf.readable(), 0);
        }

        assert_eq!(ringbuf.write(b"67890123412345").unwrap(), 11);

        let mut buf = vec![0; 7];

        assert_eq!(ringbuf.read(&mut buf).unwrap(), 7);

        assert_eq!(&buf, b"6789012");

        assert_eq!(ringbuf.writable(), 7);
        assert_eq!(ringbuf.readable(), 4);
    }
}
