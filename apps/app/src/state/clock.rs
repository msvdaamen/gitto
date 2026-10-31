//! The time relative times are told against. Ported from `hooks/relative-time.ts`.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui_kit::{App, AppContext as _, Context, Entity, Global, Task};

use crate::util::format;

/// How often relative times are brought up to date: they're told to the minute at most.
pub const TICK: Duration = Duration::from_secs(60);

/// One clock for the whole app, started with it and never stopped: a timer per row would be set
/// up again for every row that scrolls into view. Views showing a
/// relative time observe it (`cx.observe(&Clock::global(cx), |_, _, cx| cx.notify())`), so a
/// commit made "Just now" doesn't stay that until the history is reloaded.
pub struct Clock {
    ticks: u64,
    _timer: Task<()>,
}

struct GlobalClock(Entity<Clock>);

impl Global for GlobalClock {}

impl Clock {
    /// Starts the app's clock (see `state::init`).
    pub fn init(cx: &mut App) {
        let clock = cx.new(|cx: &mut Context<Clock>| Clock {
            ticks: 0,
            _timer: cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(TICK).await;
                    if this
                        .update(cx, |clock, cx| {
                            clock.ticks += 1;
                            cx.notify();
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }),
        });
        cx.set_global(GlobalClock(clock));
    }

    pub fn global(cx: &App) -> Entity<Clock> {
        cx.global::<GlobalClock>().0.clone()
    }

    pub fn ticks(&self) -> u64 {
        self.ticks
    }
}

/// Now, in milliseconds since the epoch.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as i64)
}

/// A time (milliseconds since the epoch) relative to now, e.g. "3 days ago" (`useRelativeTime`'s
/// function; observe [`Clock`] to have it told again as time passes).
pub fn relative_time(timestamp: i64) -> String {
    format::relative_time(timestamp, now_ms())
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;

    use super::*;

    // `relative-time.test.ts`: the time's told again as time passes (the telling itself is
    // `util::format::relative_time`'s, tested there).
    #[gpui_kit::test]
    fn ticks_every_minute(cx: &mut TestAppContext) {
        cx.update(Clock::init);
        let clock = cx.update(|cx| Clock::global(cx));
        assert_eq!(clock.read_with(cx, |clock, _| clock.ticks()), 0);
        cx.executor().advance_clock(TICK);
        cx.run_until_parked();
        assert_eq!(clock.read_with(cx, |clock, _| clock.ticks()), 1);
        cx.executor().advance_clock(TICK * 60);
        cx.run_until_parked();
        assert!(clock.read_with(cx, |clock, _| clock.ticks()) > 1);
    }
}
