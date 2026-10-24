//! The time relative times are told against. Ported from `hooks/relative-time.ts`.

pub use gitto_ui::hooks::relative_time::TICK;
use gpui_kit::{App, AppContext as _, Context, Entity, Global, Task};

/// One clock for the whole app, started when a relative time is first shown and never stopped: a
/// timer per row would be set up again for every row that scrolls into view. Views showing a
/// relative time observe it (`cx.observe(&Clock::global(cx), |_, _, cx| cx.notify())`), so a
/// commit made "Just now" doesn't stay that until the history is reloaded.
pub struct Clock {
    ticks: u64,
    _timer: Task<()>,
}

struct GlobalClock(Entity<Clock>);

impl Global for GlobalClock {}

impl Clock {
    pub fn global(cx: &mut App) -> Entity<Clock> {
        if let Some(clock) = cx.try_global::<GlobalClock>() {
            return clock.0.clone();
        }
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
        cx.set_global(GlobalClock(clock.clone()));
        clock
    }

    pub fn ticks(&self) -> u64 {
        self.ticks
    }
}

pub use gitto_ui::hooks::relative_time::now_ms;

/// A time (milliseconds since the epoch) relative to now, e.g. "3 days ago" (`useRelativeTime`'s
/// function; observe [`Clock`] to have it told again as time passes).
pub fn relative_time(timestamp: i64) -> String {
    gitto_ui::hooks::relative_time::relative_time_now(timestamp)
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;

    use super::*;

    // `relative-time.test.ts`: the time's told again as time passes (the telling itself is
    // `gitto_ui::format::relative_time`'s, tested there).
    #[gpui_kit::test]
    fn ticks_every_minute(cx: &mut TestAppContext) {
        let clock = cx.update(Clock::global);
        assert_eq!(clock.read_with(cx, |clock, _| clock.ticks()), 0);
        cx.executor().advance_clock(TICK);
        cx.run_until_parked();
        assert_eq!(clock.read_with(cx, |clock, _| clock.ticks()), 1);
        cx.executor().advance_clock(TICK * 60);
        cx.run_until_parked();
        assert!(clock.read_with(cx, |clock, _| clock.ticks()) > 1);
    }
}
