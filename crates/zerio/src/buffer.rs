use std::fmt::Debug;

/// Buffer for data transmission between transport layers
#[derive(PartialEq)]
pub struct Buffer {
    buf: Box<[u8]>,
    readable: usize,
}

impl Debug for Buffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Buffer")
            .field("capacity", &self.writable())
            .field("readable", &self.readable)
            .finish()
    }
}

impl Buffer {
    /// Create new buffer with specified `capacity`
    pub fn with_capacity(len: usize) -> Self {
        Self {
            buf: vec![0; len].into_boxed_slice(),
            readable: 0,
        }
    }

    /// Returns `writable` length of this buffer.
    #[inline]
    pub fn writable(&self) -> usize {
        self.buf.len()
    }

    /// Returns mutable slice point to this buffer.
    #[inline]
    pub fn writable_buf(&mut self) -> &mut [u8] {
        &mut self.buf
    }

    /// Update `writable_buf` consumed length.
    #[inline]
    pub fn writable_consume(&mut self, len: usize) {
        assert!(!(len > self.writable()), "Out of range.");
        self.readable = len;
    }

    /// Returns `readable` length of this buffer.
    #[inline]
    pub fn readable(&self) -> usize {
        self.readable
    }

    /// Returns immutable slice point to this buffer's readable range.
    #[inline]
    pub fn readable_buf(&self) -> &[u8] {
        &self.buf[..self.readable]
    }

    /// Returns mutable slice point to this buffer's readable range.
    #[inline]
    pub fn readable_buf_mut(&mut self) -> &mut [u8] {
        &mut self.buf[..self.readable]
    }
}

#[cfg(test)]
mod tests {
    use crate::Buffer;

    #[test]
    fn test_buffer() {
        let mut buf = Buffer::with_capacity(1330);

        assert_eq!(buf.readable_buf(), &[]);
        assert_eq!(buf.readable_buf_mut(), &mut []);

        assert_eq!(buf.readable(), 0);
        assert_eq!(buf.writable(), 1330);

        buf.writable_consume(1000);
        assert_eq!(buf.readable_buf(), &[0; 1000]);
        assert_eq!(buf.readable_buf().len(), 1000);
    }
}
