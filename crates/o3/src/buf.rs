//! Tiny and fixed length buffer for protocol parsing.

use std::fmt::Debug;

/// Array backed buffer,with compile-time fixed capacity.
#[derive(PartialEq)]
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

impl<const LEN: usize> ArrayBuf<LEN> {
    /// Create array buf from slice.
    ///
    /// If the input slice is greater than buf capacity, returns `None`.
    pub fn from_slice(src: &[u8]) -> Option<Self> {
        if src.len() > LEN {
            None
        } else {
            let mut buf = Self::new();

            buf.writable_buf()[..src.len()].copy_from_slice(src);
            buf.writable_consume(src.len());

            Some(buf)
        }
    }

    /// Create a new ArrayBuf with default initialization data.
    pub fn new() -> Self {
        Self {
            buf: Box::new([0; LEN]),
            len: 0,
        }
    }
    /// Returns readable data length.
    pub fn readable(&self) -> usize {
        self.len
    }

    /// Returns the capacity of this buffer.
    pub fn writable(&self) -> usize {
        return LEN;
    }

    /// Returns the whole writable buffer slice.
    pub fn writable_buf(&mut self) -> &mut [u8] {
        self.buf.as_mut_slice()
    }

    /// Update written data length.
    pub fn writable_consume(&mut self, len: usize) {
        assert!(!(len > LEN), "Out of range, {}", len);
        self.len = len;
    }

    /// Returns the whole readable buffer slice.
    pub fn readable_buf(&self) -> &[u8] {
        &self.buf.as_slice()[..self.len]
    }

    /// Returns the whole readable buffer slice.
    pub fn readable_buf_mut(&mut self) -> &mut [u8] {
        &mut self.buf.as_mut_slice()[..self.len]
    }
}

/// Fixed-length buffer for quic protocol.
pub type QuicBuf = ArrayBuf<1330>;

#[cfg(test)]
mod tests {

    use crate::buf::QuicBuf;

    #[test]
    fn test_array_buf() {
        let mut buf = QuicBuf::new();

        assert_eq!(buf.readable_buf(), &[]);
        assert_eq!(buf.readable_buf_mut(), &mut []);

        assert_eq!(buf.readable(), 0);
        assert_eq!(buf.writable(), 1330);

        buf.writable_consume(1000);
        assert_eq!(buf.readable_buf(), &[0; 1000]);
        assert_eq!(buf.readable_buf().len(), 1000);
    }
}
