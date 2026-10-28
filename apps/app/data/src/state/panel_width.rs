//! The width of a resizable panel, in GPUI's `f32` pixels. Ported from `hooks/panel-width.ts`.

use std::ops::RangeInclusive;
use std::rc::Rc;

use crate::services::LocalStorage;

/// A panel's widths, in pixels: what it starts at, and how narrow and wide it can be made.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelBounds {
    pub initial: f32,
    pub min: f32,
    pub max: f32,
}

impl PanelBounds {
    pub const fn new(initial: f32, min: f32, max: f32) -> Self {
        PanelBounds { initial, min, max }
    }

    /// How narrow and wide it can be made, as a resize handle takes it.
    pub fn range(self) -> RangeInclusive<f32> {
        self.min..=self.max
    }

    /// `width` within the bounds, in whole pixels.
    fn clamp(self, width: f64) -> f64 {
        width
            .clamp(f64::from(self.min), f64::from(self.max))
            .round()
    }
}

/// Where a panel's width is saved, by its name.
fn storage_key(name: &str) -> String {
    format!("gitto-panel-width:{name}")
}

/// The width saved as `saved` (as `localStorage` had it), within `bounds`; the initial one if
/// it's unreadable.
fn parse_width(saved: Option<&str>, bounds: PanelBounds) -> f64 {
    // As `Number()` reads it: blank is 0.
    let value = saved.and_then(|saved| match saved.trim() {
        "" => Some(0.0),
        number => number.parse::<f64>().ok(),
    });
    match value {
        Some(value) if value.is_finite() => bounds.clamp(value),
        _ => f64::from(bounds.initial),
    }
}

/// The width of a resizable panel, kept within its bounds and saved (as `localStorage` kept it)
/// so it survives restarts. Shared by all repositories: one per panel, by its `name`.
#[derive(Debug, Clone)]
pub struct PanelWidth {
    name: &'static str,
    bounds: PanelBounds,
    /// As the TS kept it: a JavaScript number, saved as one.
    width: f64,
    storage: Rc<LocalStorage>,
}

impl PanelWidth {
    pub fn new(name: &'static str, bounds: PanelBounds, storage: Rc<LocalStorage>) -> Self {
        PanelWidth {
            name,
            bounds,
            width: parse_width(storage.get_item(&storage_key(name)).as_deref(), bounds),
            storage,
        }
    }

    pub fn width(&self) -> f32 {
        self.width as f32
    }

    pub fn bounds(&self) -> PanelBounds {
        self.bounds
    }

    /// Sets the width, within the bounds; saved only once `save`, when the drag ends, rather than
    /// on every move.
    pub fn set_width(&mut self, width: f32, save: bool) {
        self.width = self.bounds.clamp(f64::from(width));
        if save {
            // Storage full or unavailable: the width still applies until the app closes.
            let _ = self
                .storage
                .set_item(&storage_key(self.name), self.width.to_string());
        }
    }

    /// Back to the initial width, saved.
    pub fn reset(&mut self) {
        self.set_width(self.bounds.initial, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOUNDS: PanelBounds = PanelBounds::new(220., 160., 480.);

    fn mount(storage: &Rc<LocalStorage>) -> PanelWidth {
        PanelWidth::new("sidebar", BOUNDS, Rc::clone(storage))
    }

    #[test]
    fn starts_at_the_initial_width() {
        let storage = Rc::new(LocalStorage::in_memory());
        assert_eq!(mount(&storage).width(), 220.);
    }

    #[test]
    fn keeps_the_width_within_its_bounds() {
        let storage = Rc::new(LocalStorage::in_memory());
        let mut panel = mount(&storage);
        panel.set_width(1000., true);
        assert_eq!(panel.width(), 480.);
        panel.set_width(20., true);
        assert_eq!(panel.width(), 160.);
    }

    #[test]
    fn keeps_the_width_across_remounts_until_its_reset() {
        let storage = Rc::new(LocalStorage::in_memory());
        mount(&storage).set_width(300.4, true);

        let mut second = mount(&storage);
        assert_eq!(second.width(), 300.);
        second.reset();

        assert_eq!(mount(&storage).width(), 220.);
    }

    #[test]
    fn doesnt_save_a_width_thats_still_being_dragged() {
        let storage = Rc::new(LocalStorage::in_memory());
        let mut first = mount(&storage);
        first.set_width(300., false);
        assert_eq!(first.width(), 300.);
        assert_eq!(mount(&storage).width(), 220.);

        let mut second = mount(&storage);
        second.set_width(300., false);
        second.set_width(310., true);
        assert_eq!(mount(&storage).width(), 310.);
    }

    #[test]
    fn ignores_unreadable_saved_widths() {
        let storage = Rc::new(LocalStorage::in_memory());
        storage
            .set_item("gitto-panel-width:sidebar", "wide")
            .unwrap();
        assert_eq!(mount(&storage).width(), 220.);
    }

    #[test]
    fn saves_widths_as_the_electron_app_did() {
        let storage = Rc::new(LocalStorage::in_memory());
        mount(&storage).set_width(300., true);
        assert_eq!(
            storage.get_item("gitto-panel-width:sidebar").as_deref(),
            Some("300")
        );
        assert_eq!(parse_width(Some(" 1e3 "), BOUNDS), 480.0);
    }
}
