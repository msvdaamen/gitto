//! A flag that only turns on once it's been on for a while. Ported from `hooks/delayed.ts`.

use std::time::Duration;

use gpui_kit::{Context, Task};

/// How long something's in progress before it's said to be, by default.
pub const DELAY: Duration = Duration::from_millis(300);

/// A value, but only true once it's been true for its delay: for showing that something's in
/// progress without flashing when it's quick. An entity: its owner [`set`](Delayed::set)s it, and
/// observes it; or a `RenderOnce` keeps one with `window.use_keyed_state`.
pub struct Delayed {
    delay: Duration,
    on: bool,
    timer: Option<Task<()>>,
}

impl Delayed {
    /// Off, then on after `delay` if `value` is true.
    pub fn new(value: bool, delay: Duration, cx: &mut Context<Self>) -> Self {
        let mut delayed = Delayed {
            delay,
            on: false,
            timer: None,
        };
        delayed.set(value, cx);
        delayed
    }

    /// Whether it's been true for the delay.
    pub fn is_on(&self) -> bool {
        self.on
    }

    pub fn set(&mut self, value: bool, cx: &mut Context<Self>) {
        if !value {
            self.timer = None;
            if self.on {
                self.on = false;
                cx.notify();
            }
            return;
        }
        if self.on || self.timer.is_some() {
            return;
        }
        let delay = self.delay;
        self.timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update(cx, |this, cx| {
                this.on = true;
                this.timer = None;
                cx.notify();
            });
        }));
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{AppContext as _, TestAppContext};

    use super::*;

    #[gpui_kit::test]
    fn turns_on_once_its_been_on_for_the_delay(cx: &mut TestAppContext) {
        let delayed = cx.new(|cx| Delayed::new(true, Duration::from_millis(150), cx));
        cx.executor().advance_clock(Duration::from_millis(149));
        cx.run_until_parked();
        assert!(!delayed.read_with(cx, |delayed, _| delayed.is_on()));
        cx.executor().advance_clock(Duration::from_millis(1));
        cx.run_until_parked();
        assert!(delayed.read_with(cx, |delayed, _| delayed.is_on()));

        delayed.update(cx, |delayed, cx| delayed.set(false, cx));
        assert!(!delayed.read_with(cx, |delayed, _| delayed.is_on()));
    }
}
