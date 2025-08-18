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
    release_timer_threshold: Duration,
) -> Option<Instant> {
    let release_time = conn
        .get_next_release_time()
        .and_then(|release| release.time(now));

    if release_time.is_some() {
        let release_time = min_of_some(conn.timeout_instant(), release_time);

        // check with `release_timer_threshold`
        release_time.filter(|time| {
            time.checked_duration_since(now).unwrap_or_default() > release_timer_threshold
        })
    } else {
        None
    }
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

#[allow(unused)]
/// Create an new random [`ConnectionId`]
pub(crate) fn random_conn_id() -> ConnectionId<'static> {
    let mut buf = vec![0; 20];
    boring::rand::rand_bytes(&mut buf).unwrap();

    ConnectionId::from_vec(buf)
}
