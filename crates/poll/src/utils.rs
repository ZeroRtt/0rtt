use std::time::{Duration, Instant};

/// Returns the minimum of `v1` and `v2`, ignoring `None`s.
pub(crate) fn min_of_some<T: Ord>(v1: Option<T>, v2: Option<T>) -> Option<T> {
    match (v1, v2) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(v), _) | (_, Some(v)) => Some(v),
        (None, None) => None,
    }
}

pub(crate) fn release_time(
    conn: &quiche::Connection,
    now: Instant,
    release_timer_threshold: Duration,
) -> Option<Instant> {
    let release_time = min_of_some(
        conn.timeout_instant(),
        conn.get_next_release_time()
            .and_then(|release| release.time(now)),
    );

    // check with `release_timer_threshold`
    release_time.filter(|time| {
        time.checked_duration_since(now).unwrap_or_default() > release_timer_threshold
    })
}
