//! A value that only follows the first and the last of quick changes. Ported from
//! `hooks/settled.ts`.

use std::time::{Duration, Instant};

use gpui_kit::{Context, Task};

/// How long a value must be quiet before a change goes through at once, by default.
pub const SETTLE_AFTER: Duration = Duration::from_millis(120);

/// A value, but while it keeps changing, only its first and its last: a change after a quiet spell
/// goes through right away, and ones that follow within `quiet` wait until it's been quiet for that
/// long. For what's costly to follow, like loading the selected commit's details while the
/// selection moves with a key held down. A change from or to `None` always goes through.
///
/// An entity: its owner [`set`](Settled::set)s it, and observes it for the settled value.
pub struct Settled<T> {
    settled: Option<T>,
    quiet: Duration,
    quiet_until: Option<Instant>,
    timer: Option<Task<()>>,
}

impl<T: Clone + PartialEq + 'static> Settled<T> {
    pub fn new(initial: Option<T>, quiet: Duration) -> Self {
        Settled {
            settled: initial,
            quiet,
            quiet_until: None,
            timer: None,
        }
    }

    /// The settled value.
    pub fn value(&self) -> Option<&T> {
        self.settled.as_ref()
    }

    /// The value changed to `next`.
    pub fn set(&mut self, next: Option<T>, cx: &mut Context<Self>) {
        self.timer = None;
        let now = cx.background_executor().now();
        let quiet = self.quiet_until.is_none_or(|until| now >= until);
        if quiet || next.is_none() || self.settled.is_none() {
            self.settle(next, cx);
        } else {
            let wait = self.quiet;
            self.timer = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(wait).await;
                let _ = this.update(cx, |this, cx| this.settle(next, cx));
            }));
        }
        self.quiet_until = Some(now + self.quiet);
    }

    fn settle(&mut self, next: Option<T>, cx: &mut Context<Self>) {
        if self.settled != next {
            self.settled = next;
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{AppContext as _, Entity, TestAppContext};

    use super::*;

    fn mount(initial: &str, cx: &mut TestAppContext) -> Entity<Settled<String>> {
        let settled =
            cx.new(|_| Settled::new(Some(initial.to_owned()), Duration::from_millis(100)));
        cx.executor().advance_clock(Duration::from_millis(500));
        settled
    }

    fn set(settled: &Entity<Settled<String>>, value: Option<&str>, cx: &mut TestAppContext) {
        settled.update(cx, |settled, cx| settled.set(value.map(str::to_owned), cx));
    }

    fn value(settled: &Entity<Settled<String>>, cx: &mut TestAppContext) -> Option<String> {
        settled.read_with(cx, |settled, _| settled.value().cloned())
    }

    fn advance(ms: u64, cx: &mut TestAppContext) {
        cx.executor().advance_clock(Duration::from_millis(ms));
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn follows_a_change_right_away_after_a_quiet_spell(cx: &mut TestAppContext) {
        let settled = mount("a", cx);
        set(&settled, Some("b"), cx);
        assert_eq!(value(&settled, cx).as_deref(), Some("b"));
    }

    #[gpui_kit::test]
    fn skips_to_the_last_of_the_changes_that_follow_quickly(cx: &mut TestAppContext) {
        let settled = mount("a", cx);
        set(&settled, Some("b"), cx);
        advance(30, cx);
        set(&settled, Some("c"), cx);
        advance(30, cx);
        set(&settled, Some("d"), cx);
        assert_eq!(value(&settled, cx).as_deref(), Some("b"));
        advance(99, cx);
        assert_eq!(value(&settled, cx).as_deref(), Some("b"));
        advance(1, cx);
        assert_eq!(value(&settled, cx).as_deref(), Some("d"));
    }

    #[gpui_kit::test]
    fn doesnt_hold_back_a_change_from_or_to_nothing(cx: &mut TestAppContext) {
        let settled = mount("a", cx);
        set(&settled, Some("b"), cx);
        set(&settled, None, cx);
        assert_eq!(value(&settled, cx), None);
        set(&settled, Some("c"), cx);
        assert_eq!(value(&settled, cx).as_deref(), Some("c"));
    }
}
