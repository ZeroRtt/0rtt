//! A lock-free and in-memory pipe implementation based on `ringbuf`.
//!
//! This pipe create a bridge between `std::io` and `futures::io`.

use std::{
    cell::UnsafeCell,
    io::{Error, ErrorKind, Read, Result, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Waker},
};

use futures::io::{AsyncRead, AsyncWrite};

use crate::RingBuf;

struct RawRingBufPipe {
    /// pipe holding ringbuf.
    ring_buf: RingBuf,
    /// waker for read task.
    reader: Option<Waker>,
    /// waker for write task.
    writer: Option<Waker>,
    /// closed flag.
    closed: bool,
}

struct RingBufPipe {
    /// spin locker for this pipe.
    locker: AtomicBool,
    /// inner impl.
    inner: UnsafeCell<RawRingBufPipe>,
}

impl RingBufPipe {
    fn lock<F, O>(&self, f: F) -> O
    where
        F: FnOnce(&mut RawRingBufPipe) -> O,
    {
        while self
            .locker
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            // No need to worry about dead cycles taking up cpu time,
            // all internal locking operations will be done in a few
            // microseconds.
        }

        let out = f(unsafe { self.inner.get().as_mut().unwrap() });

        self.locker.store(false, Ordering::Release);

        out
    }

    fn close(&self) {
        self.lock(|this| {
            this.closed = true;
            if let Some(waker) = this.writer.take() {
                waker.wake();
            }

            if let Some(waker) = this.reader.take() {
                waker.wake();
            }
        });
    }

    fn read(&self, buf: &mut [u8]) -> Result<usize> {
        self.lock(|this| {
            let read_size = this.ring_buf.read(buf)?;

            if let Some(waker) = this.writer.take() {
                waker.wake();
            }

            if read_size == 0 {
                if this.closed {
                    return Err(Error::new(ErrorKind::BrokenPipe, "pipe is closed"));
                }
                return Ok(0);
            }

            Ok(read_size)
        })
    }

    fn write(&self, buf: &[u8]) -> Result<usize> {
        self.lock(|this| {
            if this.closed {
                return Err(Error::new(ErrorKind::BrokenPipe, "pipe is closed"));
            }

            let write_size = this.ring_buf.write(buf)?;

            if let Some(waker) = this.reader.take() {
                waker.wake();
            }

            if write_size == 0 {
                return Ok(0);
            }

            Ok(write_size)
        })
    }

    fn poll_read(&self, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<Result<usize>> {
        self.lock(|this| -> Poll<Result<usize>> {
            let read_size = this.ring_buf.read(buf)?;

            if let Some(waker) = this.writer.take() {
                waker.wake();
            }

            if read_size == 0 {
                if this.closed {
                    return Poll::Ready(Ok(0));
                }

                this.reader = Some(cx.waker().clone());

                return Poll::Pending;
            }

            Poll::Ready(Ok(read_size))
        })
    }

    fn poll_write(&self, cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize>> {
        self.lock(|this| -> Poll<Result<usize>> {
            if this.closed {
                return Poll::Ready(Ok(0));
            }

            let write_size = this.ring_buf.write(buf)?;

            if let Some(waker) = this.reader.take() {
                waker.wake();
            }

            if write_size == 0 {
                this.writer = Some(cx.waker().clone());

                return Poll::Pending;
            }

            Poll::Ready(Ok(write_size))
        })
    }
}

/// Safety: protected by inner spin locker.
unsafe impl Send for RingBufPipe {}
unsafe impl Sync for RingBufPipe {}

/// `Read` part of one pipe.
pub struct PipeReader(Arc<RingBufPipe>);

impl Drop for PipeReader {
    fn drop(&mut self) {
        self.0.close();
    }
}

impl AsyncRead for PipeReader {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize>> {
        self.0.poll_read(cx, buf)
    }
}

impl Read for PipeReader {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        self.0.read(buf)
    }
}

/// `Write` part of one pipe.
pub struct PipeWriter(Arc<RingBufPipe>);

impl Drop for PipeWriter {
    fn drop(&mut self) {
        self.0.close();
    }
}

impl AsyncWrite for PipeWriter {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize>> {
        self.0.poll_write(cx, buf)
    }

    fn poll_flush(self: std::pin::Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: std::pin::Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<()>> {
        self.0.close();
        Poll::Ready(Ok(()))
    }
}

impl Write for PipeWriter {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Create a new in-memory pipeline with specified buffer length in bytes.
pub fn pipe(buffered: usize) -> (PipeWriter, PipeReader) {
    let inner = Arc::new(RingBufPipe {
        locker: Default::default(),
        inner: UnsafeCell::new(RawRingBufPipe {
            ring_buf: RingBuf::with_capacity(buffered),
            reader: Default::default(),
            writer: Default::default(),
            closed: false,
        }),
    });

    (PipeWriter(inner.clone()), PipeReader(inner))
}

#[cfg(test)]
mod tests {

    use std::io::ErrorKind;

    use futures::FutureExt;
    use futures_test::task::noop_context;

    use super::pipe;

    #[futures_test::test]
    async fn test_pipe() {
        let (mut writer, mut reader) = pipe(10);

        {
            use std::io::Write;
            assert_eq!(writer.write(b"hello world~~~~~~~~").unwrap(), 10);
        }

        {
            let mut buf = vec![0; 5];
            use futures::AsyncReadExt;
            assert_eq!(reader.read(&mut buf).await.unwrap(), 5);
            assert_eq!(&buf, b"hello");
            assert_eq!(reader.read(&mut buf).await.unwrap(), 5);
            assert_eq!(&buf, b" worl");

            assert!(
                reader
                    .read(&mut buf)
                    .poll_unpin(&mut noop_context())
                    .is_pending()
            );
        }

        {
            use futures::AsyncWriteExt;
            assert_eq!(writer.write(b"12345678900987654321").await.unwrap(), 10);

            assert!(
                writer
                    .write(b"hello")
                    .poll_unpin(&mut noop_context())
                    .is_pending()
            );
        }

        {
            use std::io::Read;
            let mut buf = vec![0; 11];
            assert_eq!(reader.read(&mut buf).unwrap(), 10);
            assert_eq!(&buf[..10], b"1234567890");
            assert_eq!(reader.read(&mut buf).unwrap(), 0);
        }

        {
            use futures::AsyncWriteExt;
            assert_eq!(writer.write(b"12345678900987654321").await.unwrap(), 10);
        }

        reader.0.close();

        {
            use std::io::Write;
            assert_eq!(
                writer.write(b"12345678900987654321").expect_err("").kind(),
                ErrorKind::BrokenPipe
            );
        }

        {
            use futures::AsyncWriteExt;
            assert_eq!(writer.write(b"12345678900987654321").await.unwrap(), 0);
        }

        {
            use std::io::Read;
            let mut buf = vec![0; 5];
            assert_eq!(reader.read(&mut buf).unwrap(), 5);
        }

        {
            use futures::AsyncReadExt;
            let mut buf = vec![0; 5];
            assert_eq!(reader.read(&mut buf).await.unwrap(), 5);
        }

        {
            use std::io::Read;
            let mut buf = vec![0; 5];
            assert_eq!(
                reader.read(&mut buf).expect_err("").kind(),
                ErrorKind::BrokenPipe
            );
        }

        {
            use futures::AsyncReadExt;
            let mut buf = vec![0; 5];
            assert_eq!(reader.read(&mut buf).await.unwrap(), 0);
        }
    }
}
