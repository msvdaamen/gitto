//! Gitto's look: `index.css`'s colors, type and radii as a GPUI Kit theme, light and dark, and the
//! theme the user picked (`hooks/theme.ts`), saved across restarts.
//!
//! Two layers, both following the theme:
//! - GPUI Kit's own theme (`cx.theme()`), which its components draw with: Gitto's colors mapped
//!   onto its roles (see [`theme_config`]), so a `Button`, a menu or a dialog looks like Gitto's.
//! - [`Palette`] (`cx.palette()`), `index.css`'s variables by name, for Gitto's own surfaces:
//!   `cx.palette().panel_raised`, `cx.palette().coral_soft`.
//!
//! Type and spacing are GPUI's rem scale (`text_sm()`, `gap_2()`, `p_4()`), on a 15px base, which
//! puts `index.css`'s sizes on steps: 11.5px metadata on `text_xs`, 12.5–13px body text on
//! `text_sm`, 15px titles on `text_base`, 22px headings on `text_2xl`. Monospace is 13px.
//!
//! Like `hooks/theme.ts`, the theme is the one the user last picked, dark until they pick one:
//! `index.css` declares `color-scheme: dark` at the root, and the renderer never read the
//! system's.

pub mod palette;

use std::rc::Rc;

use gpui_kit::component::{Colorize as _, Theme, ThemeConfig, ThemeMode};
use gpui_kit::{App, Global, Hsla, Rems, Rgba};

pub use self::palette::Palette;
use crate::services::LocalStorage;

/// The key the theme is saved under, as `hooks/theme.ts` saved it in `localStorage`.
pub const STORAGE_KEY: &str = "gitto-theme";

/// The base font size: the rem every size is relative to (see the module's doc).
pub const FONT_SIZE: f32 = 15.;

/// `px` pixels of the base font ([`FONT_SIZE`]), in rems: a TS size in pixels, kept as rems so it
/// follows the app's zoom, and the base font if that changes.
pub const fn base_px(px: f32) -> Rems {
    Rems(px / FONT_SIZE)
}
pub const MONO_FONT_SIZE: f32 = 13.;
/// Controls' corners (`rounded-[7px]`), and large surfaces' (`rounded-[11px]`).
pub const RADIUS: usize = 7;
pub const RADIUS_LG: usize = 11;

/// The app's theme (TS's `Theme`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AppTheme {
    #[default]
    Dark,
    Light,
}

impl AppTheme {
    /// As it's saved: `dark` or `light`.
    pub fn as_str(self) -> &'static str {
        match self {
            AppTheme::Dark => "dark",
            AppTheme::Light => "light",
        }
    }

    pub fn toggled(self) -> Self {
        match self {
            AppTheme::Dark => AppTheme::Light,
            AppTheme::Light => AppTheme::Dark,
        }
    }

    fn mode(self) -> ThemeMode {
        match self {
            AppTheme::Dark => ThemeMode::Dark,
            AppTheme::Light => ThemeMode::Light,
        }
    }

    /// The saved theme: light if it says so, dark otherwise (`load` in `hooks/theme.ts`).
    pub fn load(storage: &LocalStorage) -> Self {
        match storage.get_item(STORAGE_KEY).as_deref() {
            Some("light") => AppTheme::Light,
            _ => AppTheme::Dark,
        }
    }
}

/// The active theme and its palette (a GPUI global).
struct ActiveTheme {
    theme: AppTheme,
    palette: Palette,
    /// Where it's saved, if anywhere.
    storage: Option<Rc<LocalStorage>>,
}

impl Global for ActiveTheme {}

/// Reading Gitto's palette (see the module's doc).
pub trait PaletteExt {
    /// The active theme's colors.
    fn palette(&self) -> &Palette;
    /// The active theme.
    fn app_theme(&self) -> AppTheme;
}

impl PaletteExt for App {
    fn palette(&self) -> &Palette {
        &self.global::<ActiveTheme>().palette
    }

    fn app_theme(&self) -> AppTheme {
        self.global::<ActiveTheme>().theme
    }
}

/// Installs Gitto's light and dark themes and applies the one saved in `storage` (dark without
/// one). Call once, after `gpui_kit::init`.
pub fn init(storage: Option<Rc<LocalStorage>>, cx: &mut App) {
    let theme = storage.as_deref().map(AppTheme::load).unwrap_or_default();
    let inter = cx
        .text_system()
        .all_font_names()
        .iter()
        .any(|name| name == "Inter");
    Theme::update(cx, |gpui_theme| {
        gpui_theme.light_theme = Rc::new(theme_config(AppTheme::Light, inter));
        gpui_theme.dark_theme = Rc::new(theme_config(AppTheme::Dark, inter));
    });
    cx.set_global(ActiveTheme {
        theme,
        palette: Palette::of(theme),
        storage,
    });
    apply(theme, cx);
}

/// Switches to `theme`, and saves it; it still applies if it can't be saved, until the app closes
/// (`applyTheme` in `hooks/theme.ts`).
pub fn set(theme: AppTheme, cx: &mut App) {
    let active = cx.global_mut::<ActiveTheme>();
    active.theme = theme;
    active.palette = Palette::of(theme);
    if let Some(storage) = &active.storage {
        let _ = storage.set_item(STORAGE_KEY, theme.as_str());
    }
    apply(theme, cx);
}

/// Switches between the dark and the light theme (`toggleTheme`).
pub fn toggle(cx: &mut App) {
    set(cx.app_theme().toggled(), cx);
}

fn apply(theme: AppTheme, cx: &mut App) {
    Theme::change(theme.mode(), None, cx);
}

/// How many of GPUI Kit's color roles Gitto sets.
const COLOR_COUNT: usize = 107;

/// Gitto's colors (see [`Palette`]) as GPUI Kit's theme roles, for `theme`; with Inter as the
/// font where it's installed (`--font-sans`), the system's otherwise.
pub fn theme_config(theme: AppTheme, inter: bool) -> ThemeConfig {
    let p = Palette::of(theme);
    let hex = |color: Hsla| color.to_hex();
    let transparent = Hsla::default().to_hex();
    let primary_foreground = match theme {
        AppTheme::Dark => p.primary_button_text,
        AppTheme::Light => p.avatar_text,
    };
    let mut scrollbar_thumb = p.border;
    scrollbar_thumb.a = 0.65;
    let mut selection = p.primary;
    selection.a = 0.35;
    let colors: [(&str, String); COLOR_COUNT] = [
        ("background", hex(p.bg)),
        ("foreground", hex(p.text)),
        ("border", hex(p.border)),
        ("input.border", hex(p.border)),
        ("ring", hex(p.primary)),
        ("caret", hex(p.text)),
        ("selection.background", hex(selection)),
        ("overlay", hex(p.overlay)),
        ("window.border", hex(p.border)),
        ("muted.background", hex(p.panel_raised)),
        ("muted.foreground", hex(p.muted)),
        ("accent.background", hex(p.panel_hover)),
        ("accent.foreground", hex(p.text)),
        ("popover.background", hex(p.panel_raised)),
        ("popover.foreground", hex(p.text)),
        ("group_box.background", hex(p.panel)),
        ("group_box.foreground", hex(p.text)),
        ("accordion.background", hex(p.panel)),
        ("skeleton.background", hex(p.panel_raised)),
        ("primary.background", hex(p.primary)),
        ("primary.hover.background", hex(p.primary_strong)),
        ("primary.active.background", hex(p.primary_strong)),
        ("primary.foreground", hex(primary_foreground)),
        ("secondary.background", hex(p.panel_raised)),
        ("secondary.hover.background", hex(p.panel_hover)),
        ("secondary.active.background", hex(p.panel_active)),
        ("secondary.foreground", hex(p.text)),
        ("button.background", hex(p.panel_raised)),
        ("button.hover.background", hex(p.panel_hover)),
        ("button.active.background", hex(p.panel_active)),
        ("button.foreground", hex(p.text)),
        ("button.primary.background", hex(p.primary_button_end)),
        (
            "button.primary.hover.background",
            hex(p.primary_button_start),
        ),
        (
            "button.primary.active.background",
            hex(p.primary_button_end),
        ),
        ("button.primary.foreground", hex(p.primary_button_text)),
        ("danger.background", hex(p.coral)),
        ("danger.hover.background", hex(p.coral)),
        ("danger.active.background", hex(p.coral)),
        ("danger.foreground", hex(p.bg)),
        ("warning.background", hex(p.amber)),
        ("warning.hover.background", hex(p.amber)),
        ("warning.active.background", hex(p.amber)),
        ("warning.foreground", hex(p.bg)),
        ("success.background", hex(p.mint)),
        ("success.hover.background", hex(p.mint)),
        ("success.active.background", hex(p.mint)),
        ("success.foreground", hex(p.bg)),
        ("info.background", hex(p.blue)),
        ("info.hover.background", hex(p.blue)),
        ("info.active.background", hex(p.blue)),
        ("info.foreground", hex(p.bg)),
        ("link", hex(p.primary_strong)),
        ("link.hover", hex(p.primary)),
        ("link.active", hex(p.primary_strong)),
        ("list.background", hex(p.panel)),
        ("list.even.background", hex(p.panel)),
        ("list.head.background", hex(p.panel)),
        ("list.hover.background", hex(p.panel_hover)),
        ("list.active.background", hex(p.primary_soft)),
        ("list.active.border", hex(p.primary)),
        ("table.background", hex(p.bg)),
        ("table.even.background", hex(p.bg)),
        ("table.head.background", hex(p.bg_soft)),
        ("table.head.foreground", hex(p.muted)),
        ("table.hover.background", hex(p.panel_hover)),
        ("table.active.background", hex(p.primary_soft)),
        ("table.active.border", hex(p.primary)),
        ("table.row.border", hex(p.border_soft)),
        ("sidebar.background", hex(p.panel)),
        ("sidebar.foreground", hex(p.text)),
        ("sidebar.border", hex(p.border)),
        ("sidebar.accent.background", hex(p.panel_hover)),
        ("sidebar.accent.foreground", hex(p.text)),
        ("sidebar.primary.background", hex(p.primary_soft)),
        ("sidebar.primary.foreground", hex(p.primary_strong)),
        ("tab_bar.background", hex(p.header)),
        ("tab_bar.segmented.background", hex(p.bg)),
        ("tab.background", transparent.clone()),
        ("tab.foreground", hex(p.muted)),
        ("tab.active.background", hex(p.bg)),
        ("tab.active.foreground", hex(p.text)),
        ("title_bar.background", hex(p.header)),
        ("title_bar.border", hex(p.border)),
        ("status_bar.background", hex(p.footer)),
        ("status_bar.border", hex(p.border)),
        ("switch.background", hex(p.border)),
        ("switch.thumb.background", hex(p.avatar_text)),
        ("slider.background", hex(p.primary)),
        ("slider.thumb.background", hex(p.avatar_text)),
        ("progress.bar.background", hex(p.primary)),
        ("scrollbar.background", transparent),
        ("scrollbar.thumb.background", hex(scrollbar_thumb)),
        ("scrollbar.thumb.hover.background", hex(p.faint)),
        ("drag.border", hex(p.primary)),
        ("drop_target.background", hex(p.primary_soft)),
        ("base.red", hex(p.coral)),
        ("base.red.light", hex(p.coral_soft)),
        ("base.green", hex(p.mint)),
        ("base.green.light", hex(p.mint_soft)),
        ("base.yellow", hex(p.amber)),
        ("base.yellow.light", hex(p.amber_soft)),
        ("base.blue", hex(p.blue)),
        ("base.blue.light", hex(p.blue_soft)),
        ("base.magenta", hex(p.primary)),
        ("base.magenta.light", hex(p.primary_soft)),
        ("base.cyan", hex(p.blue)),
        ("base.cyan.light", hex(p.blue_soft)),
    ];
    let colors: serde_json::Map<String, serde_json::Value> = colors
        .into_iter()
        .map(|(role, color)| (role.to_owned(), color.into()))
        .collect();
    let syntax: serde_json::Map<String, serde_json::Value> = palette::syntax_colors(theme)
        .into_iter()
        .map(|(name, color)| (name.to_owned(), serde_json::json!({ "color": color })))
        .collect();
    let mut config = serde_json::json!({
        "name": match theme {
            AppTheme::Dark => "Gitto Dark",
            AppTheme::Light => "Gitto Light",
        },
        "mode": theme.as_str(),
        "font.size": FONT_SIZE,
        "mono_font.size": MONO_FONT_SIZE,
        "radius": RADIUS,
        "radius.lg": RADIUS_LG,
        "shadow": true,
        "colors": colors,
        // Without, code is drawn with GPUI Kit's light colors in either theme. The code editor
        // (editing a file in its diff) on the diff's background, its line numbers as the diff's.
        "highlight": {
            "syntax": syntax,
            "editor_background": hex(p.bg),
            "editor.gutter.background": hex(p.bg),
            "editor_line_number": hex(p.faint),
            "editor_active_line_number": hex(p.text_soft),
        },
    });
    if inter {
        config["font.family"] = "Inter".into();
    }
    serde_json::from_value(config).expect("Gitto's theme is a GPUI Kit theme")
}

/// `color-mix(in srgb, color share%, other)`: the two mixed channel by channel in sRGB, as TS's
/// borders mixed a color into the border's.
pub fn mixed(color: Hsla, share: f32, other: Hsla) -> Hsla {
    let a = Rgba::from(color);
    let b = Rgba::from(other);
    let mix = |x: f32, y: f32| x * share + y * (1. - share);
    Rgba {
        r: mix(a.r, b.r),
        g: mix(a.g, b.g),
        b: mix(a.b, b.b),
        a: mix(a.a, b.a),
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_a_ts_pixel_measure_by_the_base_font() {
        const ROW: Rems = base_px(30.);
        assert_eq!(ROW, Rems(2.));
        assert_eq!(base_px(FONT_SIZE), Rems(1.));
    }

    #[test]
    fn mixes_colors_channel_by_channel() {
        let red: Hsla = gpui_kit::rgb(0xff0000).into();
        let blue: Hsla = gpui_kit::rgb(0x0000ff).into();
        let half = Rgba::from(mixed(red, 0.5, blue));
        assert!((half.r - 0.5).abs() < 0.01 && (half.b - 0.5).abs() < 0.01);
        assert_eq!(mixed(red, 1., blue), red);
    }

    // `theme.test.ts`, on what keeps the theme rather than the DOM.

    #[test]
    fn starts_from_the_saved_theme_and_saves_a_toggle() {
        let storage = LocalStorage::in_memory();
        storage.set_item(STORAGE_KEY, "light").unwrap();

        let theme = AppTheme::load(&storage);
        assert_eq!(theme, AppTheme::Light);
        let toggled = theme.toggled();
        storage.set_item(STORAGE_KEY, toggled.as_str()).unwrap();
        assert_eq!(toggled, AppTheme::Dark);
        assert_eq!(storage.get_item(STORAGE_KEY).as_deref(), Some("dark"));
    }

    #[test]
    fn falls_back_to_dark() {
        assert_eq!(AppTheme::load(&LocalStorage::in_memory()), AppTheme::Dark);
        let storage = LocalStorage::in_memory();
        storage.set_item(STORAGE_KEY, "sepia").unwrap();
        assert_eq!(AppTheme::load(&storage), AppTheme::Dark);
    }

    #[test]
    fn makes_a_gpui_kit_theme_of_each() {
        let dark = theme_config(AppTheme::Dark, false);
        assert_eq!(dark.mode, ThemeMode::Dark);
        assert_eq!(dark.font_size, Some(FONT_SIZE));
        assert_eq!(dark.font_family, None);
        let light = theme_config(AppTheme::Light, true);
        assert_eq!(light.mode, ThemeMode::Light);
        assert_eq!(light.font_family.as_deref(), Some("Inter"));
        assert_eq!(
            light.colors.background.as_deref(),
            Some(Palette::light().bg.to_hex().as_str())
        );
    }
}

#[cfg(test)]
mod ui_tests {
    use gpui_kit::TestAppContext;

    use super::*;

    #[gpui_kit::test]
    fn still_toggles_without_storage(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| init(None, cx));
        assert_eq!(cx.update(|cx| cx.app_theme()), AppTheme::Dark);
        assert!(cx.update(|cx| Theme::global(cx).is_dark()));

        cx.update(toggle);
        assert_eq!(cx.update(|cx| cx.app_theme()), AppTheme::Light);
        assert!(!cx.update(|cx| Theme::global(cx).is_dark()));
        assert_eq!(cx.update(|cx| cx.theme_background()), Palette::light().bg);
    }

    #[gpui_kit::test]
    fn starts_from_the_saved_theme(cx: &mut TestAppContext) {
        let storage = Rc::new(LocalStorage::in_memory());
        storage.set_item(STORAGE_KEY, "light").unwrap();
        cx.update(gpui_kit::init);
        cx.update(|cx| init(Some(Rc::clone(&storage)), cx));
        assert_eq!(cx.update(|cx| cx.app_theme()), AppTheme::Light);
        cx.update(toggle);
        assert_eq!(storage.get_item(STORAGE_KEY).as_deref(), Some("dark"));
    }

    trait Background {
        fn theme_background(&self) -> Hsla;
    }

    impl Background for App {
        fn theme_background(&self) -> Hsla {
            Theme::global(self).background
        }
    }
}
