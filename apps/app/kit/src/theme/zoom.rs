//! How big the UI is drawn: the View menu's zoom, as Electron's page zoom was, saved across
//! restarts. GPUI has no page zoom, but every size on the rem scale is relative to the theme's
//! font size, which GPUI Kit makes the window's rem: zooming scales that ([`FONT_SIZE`] and
//! [`MONO_FONT_SIZE`] times the zoom), and the UI with it. Sizes in pixels (the window's, the
//! panels' widths as dragged) stay as they are.

use std::rc::Rc;

use gpui_kit::component::{Theme, ThemeConfig};
use gpui_kit::{App, Global};

use super::{FONT_SIZE, MONO_FONT_SIZE};

/// The key the zoom is saved under.
pub const STORAGE_KEY: &str = "gitto-zoom";

/// The zooms Zoom In and Zoom Out step through, as a browser's.
pub const ZOOMS: [f32; 11] = [0.5, 0.67, 0.75, 0.8, 0.9, 1., 1.1, 1.25, 1.5, 1.75, 2.];

/// Saves the zoom, as it's saved: e.g. `1.25`.
pub type SaveZoom = Box<dyn Fn(&str)>;

/// The zoom, and what saves it (a GPUI global).
struct Zoom {
    zoom: f32,
    save: Option<SaveZoom>,
}

impl Global for Zoom {}

/// Applies the zoom saved as `saved` (none without one, or with one that isn't a step of
/// [`ZOOMS`]); `save` saves it as it changes from then on. Call once, after `theme::init`.
pub fn init(saved: Option<&str>, save: Option<SaveZoom>, cx: &mut App) {
    let zoom = saved
        .and_then(|saved| saved.trim().parse::<f32>().ok())
        .filter(|zoom| ZOOMS.contains(zoom))
        .unwrap_or(1.);
    cx.set_global(Zoom { zoom, save });
    apply(zoom, cx);
}

/// The zoom: 1 for the UI at its size.
pub fn zoom(cx: &App) -> f32 {
    cx.try_global::<Zoom>().map_or(1., |zoom| zoom.zoom)
}

/// Zooms in a step, up to the last of [`ZOOMS`].
pub fn zoom_in(cx: &mut App) {
    let zoom = zoom(cx);
    if let Some(&next) = ZOOMS.iter().find(|step| **step > zoom) {
        set(next, cx);
    }
}

/// Zooms out a step, down to the first of [`ZOOMS`].
pub fn zoom_out(cx: &mut App) {
    let zoom = zoom(cx);
    if let Some(&next) = ZOOMS.iter().rev().find(|step| **step < zoom) {
        set(next, cx);
    }
}

/// Draws the UI at its size again.
pub fn reset(cx: &mut App) {
    set(1., cx);
}

fn set(zoom: f32, cx: &mut App) {
    if !cx.has_global::<Zoom>() {
        cx.set_global(Zoom { zoom, save: None });
    }
    let state = cx.global_mut::<Zoom>();
    if state.zoom == zoom {
        return;
    }
    state.zoom = zoom;
    if let Some(save) = &state.save {
        save(&zoom.to_string());
    }
    apply(zoom, cx);
}

/// Sizes both themes' type for `zoom`, and applies the theme on show again.
fn apply(zoom: f32, cx: &mut App) {
    let sized = |config: &Rc<ThemeConfig>| {
        let mut config = ThemeConfig::clone(config);
        config.font_size = Some(FONT_SIZE * zoom);
        config.mono_font_size = Some(MONO_FONT_SIZE * zoom);
        Rc::new(config)
    };
    let mode = Theme::update(cx, |theme| {
        theme.light_theme = sized(&theme.light_theme);
        theme.dark_theme = sized(&theme.dark_theme);
        theme.mode
    });
    Theme::change(mode, None, cx);
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use gpui_kit::TestAppContext;
    use gpui_kit::px;

    use super::*;
    use crate::theme::{self, AppTheme};

    fn font_size(cx: &mut TestAppContext) -> gpui_kit::Pixels {
        cx.update(|cx| Theme::global(cx).font_size)
    }

    #[gpui_kit::test]
    fn zooms_the_ui_by_its_font_size_a_step_at_a_time(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| theme::init(None, None, cx));
        let saved = Rc::new(RefCell::new(Vec::new()));
        let save: SaveZoom = Box::new({
            let saved = Rc::clone(&saved);
            move |zoom| saved.borrow_mut().push(zoom.to_owned())
        });
        cx.update(|cx| init(None, Some(save), cx));
        assert_eq!(font_size(cx), px(FONT_SIZE));

        cx.update(zoom_in);
        cx.update(zoom_in);
        assert_eq!(font_size(cx), px(FONT_SIZE * 1.25));
        assert_eq!(
            cx.update(|cx| Theme::global(cx).mono_font_size),
            px(MONO_FONT_SIZE * 1.25)
        );
        // Still, in the other theme.
        cx.update(|cx| theme::set(AppTheme::Light, cx));
        assert_eq!(font_size(cx), px(FONT_SIZE * 1.25));

        cx.update(zoom_out);
        assert_eq!(font_size(cx), px(FONT_SIZE * 1.1));
        cx.update(reset);
        assert_eq!(font_size(cx), px(FONT_SIZE));
        assert_eq!(*saved.borrow(), ["1.1", "1.25", "1.1", "1"]);
    }

    #[gpui_kit::test]
    fn stops_at_the_last_step(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| theme::init(None, None, cx));
        cx.update(|cx| init(Some("2"), None, cx));
        cx.update(zoom_in);
        assert_eq!(cx.update(|cx| zoom(cx)), 2.);
        cx.update(|cx| init(Some("0.5"), None, cx));
        cx.update(zoom_out);
        assert_eq!(cx.update(|cx| zoom(cx)), 0.5);
    }

    #[gpui_kit::test]
    fn starts_from_the_saved_zoom_if_its_a_step(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| theme::init(None, None, cx));
        cx.update(|cx| init(Some("1.5"), None, cx));
        assert_eq!(font_size(cx), px(FONT_SIZE * 1.5));
        cx.update(|cx| init(Some("7"), None, cx));
        assert_eq!(font_size(cx), px(FONT_SIZE));
    }
}
