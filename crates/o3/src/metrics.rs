//! Metrics utilities for o3 projects.

use std::task::Poll;

use metricrs::{Counter, Token, global::get_global_registry};
use pin_project::pin_project;
use tokio::io::AsyncWrite;

/// Add metrics functions for [`AsyncWrite`]
#[pin_project]
pub struct AsyncMetricWrite<W> {
    #[pin]
    write: W,
    counter: Option<Counter>,
}

impl<W> AsyncMetricWrite<W> {
    /// Creata a new [`AsyncMetricWrite`]
    pub fn new(write: W, name: &str, labels: &[(&str, &str)]) -> Self {
        Self {
            write,
            counter: get_global_registry()
                .map(|registry| registry.counter(Token::new(name, labels))),
        }
    }
}

impl<W> AsyncWrite for AsyncMetricWrite<W>
where
    W: AsyncWrite,
{
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        let this = self.project();

        match this.write.poll_write(cx, buf) {
            std::task::Poll::Ready(Ok(send_size)) => {
                // update metrics instrument `COUNTER`.
                if let Some(counter) = &this.counter {
                    counter.increment(send_size as u64);
                }

                return Poll::Ready(Ok(send_size));
            }
            poll => poll,
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        self.project().write.poll_flush(cx)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        self.project().write.poll_shutdown(cx)
    }
}
