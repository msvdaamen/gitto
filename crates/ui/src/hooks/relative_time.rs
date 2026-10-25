//! Ported from `hooks/relative-time.ts`: the clock relative times are told by. The TS hook's
//! signal is the app's to hold; this is how often it's to tick, and the time it tells.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::format::relative_time;

/// How often relative times are brought up to date: they're told to the minute at most.
///
/// Run one clock for the whole app, started when a relative time is first shown and never
/// stopped: a timer per row would be set up again for every row that scrolls into view. What shows
/// a relative time is told it again on each tick, so a commit made "Just now" doesn't stay that
/// until the history is reloaded.
pub const TICK: Duration = Duration::from_secs(60);

/// Milliseconds since the epoch, now, as `Date.now()`.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as i64)
}

/// Tells a time (milliseconds since the epoch) relative to now, e.g. "3 days ago"; the time itself
/// is always the current one, whichever tick it's told on.
pub fn relative_time_now(timestamp: i64) -> String {
    relative_time(timestamp, now_ms())
}

#[cfg(test)]
mod tests {
    use super::*;

    mod use_relative_time {
        use super::*;

        #[test]
        fn tells_a_time_again_as_time_passes() {
            let mut now = now_ms();
            let made = now - 20_000;
            assert_eq!(relative_time(made, now), "Just now");
            assert_eq!(relative_time_now(made), "Just now");

            now += TICK.as_millis() as i64;
            assert_eq!(relative_time(made, now), "1 minute ago");
            now += 60 * 60_000;
            assert_eq!(relative_time(made, now), "1 hour ago");
        }
    }
}
