use std::task::Poll;

use futures::FutureExt;
use futures_test::task::noop_context;
use metricrs::counter;

#[test]
fn derive_counter() {
    #[counter("test.mock_send")]
    fn mock_send() -> usize {
        1
    }

    assert_eq!(mock_send(), 1);

    struct Mock;

    impl Mock {
        #[counter("test.mock.async_send", class = "Mock")]
        async fn send(&mut self) -> usize {
            1
        }
    }

    assert_eq!(
        Box::pin(Mock.send()).poll_unpin(&mut noop_context()),
        Poll::Ready(1)
    );
}
