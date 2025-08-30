//! Tiny and fixed length buffer for protocol parsing.

/// Array backed buffer,with compile-time fixed capacity.
pub struct ArrayBuf<const LEN: usize> {
    /// fixed array buffer.
    buf: Box<[u8; LEN]>,
    /// written data length.
    len: usize,
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
