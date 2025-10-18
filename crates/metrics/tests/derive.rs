use std::task::Poll;

use futures::FutureExt;
use futures_test::task::noop_context;
use metricrs::instrument;

#[test]
fn instrument() {
    // _ = pretty_env_logger::try_init();

    #[instrument(kind = Counter, name = "test.mock_send")]
    fn mock_send() -> usize {
        1
    }

    assert_eq!(mock_send(), 1);

    struct Mock;

    impl Mock {
        #[instrument(kind = Timer, name = "test.mock.async_send")]
        #[instrument(kind = Counter, name = "test.mock_send")]
        async fn send(&mut self) -> usize {
            1
        }
    }

    assert_eq!(
        Box::pin(Mock.send()).poll_unpin(&mut noop_context()),
        Poll::Ready(1)
    );
}
