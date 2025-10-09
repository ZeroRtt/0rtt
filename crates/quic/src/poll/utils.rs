use std::time::{Duration, Instant};

use quiche::ConnectionId;

/// Returns the minimum of `v1` and `v2`, ignoring `None`s.
pub(crate) fn min_of_some<T: Ord>(v1: Option<T>, v2: Option<T>) -> Option<T> {
    match (v1, v2) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(v), _) | (_, Some(v)) => Some(v),
        (None, None) => None,
    }
}

pub(crate) fn delay_send(
    conn: &quiche::Connection,
    now: Instant,
    send_done: bool,
) -> Option<Instant> {
    let release_time = conn
        .get_next_release_time()
        .and_then(|release| release.time(now));

    let release_time = if send_done {
        min_of_some(conn.timeout_instant(), release_time)
    } else {
        if release_time.is_some() {
            min_of_some(conn.timeout_instant(), release_time)
        } else {
            None
        }
    };

    log::trace!(
        "id={}, next_release_time={:?}",
        conn.trace_id(),
        release_time
    );

    release_time
}

pub(crate) fn release_time(
    conn: &quiche::Connection,
    now: Instant,
    release_timer_threshold: Duration,
) -> Option<Instant> {
    // check with `release_timer_threshold`
    conn.get_next_release_time()
        .and_then(|release| release.time(now))
        .filter(|time| {
            time.checked_duration_since(now).unwrap_or_default() > release_timer_threshold
        })
}

/// Create an new random [`ConnectionId`]
pub(crate) fn random_conn_id() -> ConnectionId<'static> {
    let mut buf = vec![0; 20];
    boring::rand::rand_bytes(&mut buf).unwrap();

    ConnectionId::from_vec(buf)
}

/// Returns true if the stream was created locally.
pub(crate) fn is_local(stream_id: u64, is_server: bool) -> bool {
    (stream_id & 0x1) == (is_server as u64)
}

/// Returns true if the stream is bidirectional.
pub(crate) fn is_bidi(stream_id: u64) -> bool {
    (stream_id & 0x2) == 0
}
