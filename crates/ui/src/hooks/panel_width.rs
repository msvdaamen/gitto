//! Ported from `hooks/panel-width.ts`: the width of a resizable panel.

use super::Storage;

/// The width a panel starts at, and the narrowest and widest it can be.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelBounds {
    pub initial: f64,
    pub min: f64,
    pub max: f64,
}

/// Where a panel's width is saved, by its name.
pub fn storage_key(name: &str) -> String {
    format!("gitto-panel-width:{name}")
}

/// `width` within `bounds`, in whole pixels.
pub fn clamp(width: f64, bounds: PanelBounds) -> f64 {
    width.clamp(bounds.min, bounds.max).round()
}

/// The width saved as `saved` (as `localStorage` had it), within `bounds`; the initial one if
/// it's unreadable.
pub fn parse_width(saved: Option<&str>, bounds: PanelBounds) -> f64 {
    // As `Number()` reads it: blank is 0.
    let value = saved.and_then(|saved| match saved.trim() {
        "" => Some(0.0),
        number => number.parse::<f64>().ok(),
    });
    match value {
        Some(value) if value.is_finite() => clamp(value, bounds),
        _ => bounds.initial,
    }
}

/// The width of a resizable panel, kept within `bounds` and saved in a [`Storage`] so it survives
/// restarts. Shared by all repositories.
#[derive(Debug, Clone, PartialEq)]
pub struct PanelWidth {
    name: String,
    bounds: PanelBounds,
    width: f64,
}

impl PanelWidth {
    /// The width saved for the panel `name`, or its initial one.
    pub fn load(name: &str, bounds: PanelBounds, storage: &dyn Storage) -> Self {
        Self {
            name: name.to_owned(),
            bounds,
            width: parse_width(storage.get(&storage_key(name)).as_deref(), bounds),
        }
    }

    pub fn width(&self) -> f64 {
        self.width
    }

    pub fn bounds(&self) -> PanelBounds {
        self.bounds
    }

    /// Sets the width, and saves it in `storage`; `None` while it's being dragged, which sets it
    /// on every frame.
    pub fn set_width(&mut self, next: f64, storage: Option<&mut dyn Storage>) {
        self.width = clamp(next, self.bounds);
        let Some(storage) = storage else { return };
        // Storage full or unavailable: the width still applies until the app closes.
        let _ = storage.set(&storage_key(&self.name), &self.width.to_string());
    }

    /// Back to the initial width, saved.
    pub fn reset(&mut self, storage: &mut dyn Storage) {
        self.set_width(self.bounds.initial, Some(storage));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::MemoryStorage;

    const BOUNDS: PanelBounds = PanelBounds {
        initial: 220.0,
        min: 160.0,
        max: 480.0,
    };

    /// Loads the width, like a freshly mounted layout.
    fn mount(storage: &MemoryStorage) -> PanelWidth {
        PanelWidth::load("sidebar", BOUNDS, storage)
    }

    mod use_panel_width {
        use super::*;

        #[test]
        fn starts_at_the_initial_width() {
            let panel = mount(&MemoryStorage::default());

            assert_eq!(panel.width(), 220.0);
        }

        #[test]
        fn keeps_the_width_within_its_bounds() {
            let mut storage = MemoryStorage::default();
            let mut panel = mount(&storage);

            panel.set_width(1000.0, Some(&mut storage));
            assert_eq!(panel.width(), 480.0);
            panel.set_width(20.0, Some(&mut storage));
            assert_eq!(panel.width(), 160.0);
        }

        #[test]
        fn keeps_the_width_across_remounts_until_its_reset() {
            let mut storage = MemoryStorage::default();
            let mut first = mount(&storage);
            first.set_width(300.4, Some(&mut storage));

            let mut second = mount(&storage);
            assert_eq!(second.width(), 300.0);
            second.reset(&mut storage);

            let third = mount(&storage);
            assert_eq!(third.width(), 220.0);
        }

        #[test]
        fn doesnt_save_a_width_thats_still_being_dragged() {
            let mut storage = MemoryStorage::default();
            let mut first = mount(&storage);
            first.set_width(300.0, None);
            assert_eq!(first.width(), 300.0);
            assert_eq!(mount(&storage).width(), 220.0);

            let mut second = mount(&storage);
            second.set_width(300.0, None);
            second.set_width(310.0, Some(&mut storage));
            assert_eq!(mount(&storage).width(), 310.0);
        }

        #[test]
        fn ignores_unreadable_saved_widths() {
            let mut storage = MemoryStorage::default();
            storage.set("gitto-panel-width:sidebar", "wide").unwrap();
            let panel = mount(&storage);

            assert_eq!(panel.width(), 220.0);
        }
    }

    #[test]
    fn saves_widths_as_the_electron_app_did() {
        let mut storage = MemoryStorage::default();
        mount(&storage).set_width(300.0, Some(&mut storage));
        assert_eq!(
            storage.get("gitto-panel-width:sidebar").as_deref(),
            Some("300")
        );
        assert_eq!(parse_width(Some(" 1e3 "), BOUNDS), 480.0);
    }
}
