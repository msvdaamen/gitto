//! The width of a resizable panel: gitto-ui's `hooks::panel_width` (ported from
//! `hooks/panel-width.ts`), saved in the app's [`LocalStorage`], in GPUI's `f32` pixels.

use std::rc::Rc;

use gitto_ui::hooks::panel_width;

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

    fn of_hook(self) -> panel_width::PanelBounds {
        panel_width::PanelBounds {
            initial: f64::from(self.initial),
            min: f64::from(self.min),
            max: f64::from(self.max),
        }
    }
}

/// The width of a resizable panel, kept within its bounds and saved (as `localStorage` kept it)
/// so it survives restarts. Shared by all repositories: one per panel, by its `name`.
#[derive(Debug, Clone)]
pub struct PanelWidth {
    bounds: PanelBounds,
    width: panel_width::PanelWidth,
    storage: Rc<LocalStorage>,
}

impl PanelWidth {
    pub fn new(name: &'static str, bounds: PanelBounds, storage: Rc<LocalStorage>) -> Self {
        PanelWidth {
            bounds,
            width: panel_width::PanelWidth::load(name, bounds.of_hook(), &&*storage),
            storage,
        }
    }

    pub fn width(&self) -> f32 {
        self.width.width() as f32
    }

    pub fn bounds(&self) -> PanelBounds {
        self.bounds
    }

    /// Sets the width, within the bounds; saved only once `save`, when the drag ends, rather than
    /// on every move.
    pub fn set_width(&mut self, width: f32, save: bool) {
        let mut storage = &*self.storage;
        self.width.set_width(
            f64::from(width),
            save.then_some(&mut storage as &mut dyn gitto_ui::hooks::Storage),
        );
    }

    pub fn reset(&mut self) {
        self.width.reset(&mut &*self.storage);
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
        storage.set_item("gitto-panel-width:sidebar", "wide");
        assert_eq!(mount(&storage).width(), 220.);
    }
}
