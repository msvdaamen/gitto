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

pub mod breakpoints;
pub mod palette;

use std::rc::Rc;

use gpui_kit::component::highlighter::HighlightThemeStyle;
use gpui_kit::component::{Colorize as _, Theme, ThemeConfig, ThemeConfigColors, ThemeMode};
use gpui_kit::{App, Global, Hsla, Rems, Rgba};

pub use self::palette::Palette;
use crate::services::LocalStorage;

/// The key the theme is saved under, as `hooks/theme.ts` saved it in `localStorage`.
pub const STORAGE_KEY: &str = "gitto-theme";

/// The base font size: the rem every size is relative to (see the module's doc).
pub const FONT_SIZE: f32 = 15.;

pub const MONO_FONT_SIZE: f32 = 13.;

/// `px` of `index.css`'s, in rems of the [`FONT_SIZE`] base: what a size with no step on the rem
/// scale is written as, e.g. `.h(base_px(38.))`.
pub const fn base_px(px: f32) -> Rems {
    Rems(px / FONT_SIZE)
}

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

/// The two themes' palettes, and where the theme's saved (a GPUI global). Which one's active is
/// GPUI Kit's theme mode: anything that changes it (`Theme::change`) changes Gitto's palette too.
struct ActiveTheme {
    dark: Palette,
    light: Palette,
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
        let active = self.global::<ActiveTheme>();
        match self.app_theme() {
            AppTheme::Dark => &active.dark,
            AppTheme::Light => &active.light,
        }
    }

    fn app_theme(&self) -> AppTheme {
        match Theme::global(self).mode {
            ThemeMode::Dark => AppTheme::Dark,
            ThemeMode::Light => AppTheme::Light,
        }
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
        dark: Palette::dark(),
        light: Palette::light(),
        storage,
    });
    Theme::change(theme.mode(), None, cx);
}

/// Switches to `theme`, and saves it; it still applies if it can't be saved, until the app closes
/// (`applyTheme` in `hooks/theme.ts`).
pub fn set(theme: AppTheme, cx: &mut App) {
    Theme::change(theme.mode(), None, cx);
    if let Some(storage) = &cx.global::<ActiveTheme>().storage {
        storage.set_item(STORAGE_KEY, theme.as_str());
    }
}

/// Switches between the dark and the light theme (`toggleTheme`).
pub fn toggle(cx: &mut App) {
    set(cx.app_theme().toggled(), cx);
}

/// Sets each color role named (a field of GPUI Kit's `ThemeConfigColors`, so one misspelled
/// doesn't compile) to its color.
macro_rules! set_roles {
    ($colors:ident { $($role:ident: $color:expr),* $(,)? }) => {
        $( $colors.$role = Some($color.to_hex().into()); )*
    };
}

/// One of a palette's colors.
type PaletteColor = fn(&Palette) -> Hsla;

/// GPUI Kit's base colors, which its components fall back on, by the names its theme files give
/// them: their fields are private, so only a theme file's names set them. With the palette's
/// color each is.
const BASE_COLORS: [(&str, PaletteColor); 12] = [
    ("base.red", |p| p.coral),
    ("base.red.light", |p| p.coral_soft),
    ("base.green", |p| p.mint),
    ("base.green.light", |p| p.mint_soft),
    ("base.yellow", |p| p.amber),
    ("base.yellow.light", |p| p.amber_soft),
    ("base.blue", |p| p.blue),
    ("base.blue.light", |p| p.blue_soft),
    ("base.magenta", |p| p.primary),
    ("base.magenta.light", |p| p.primary_soft),
    ("base.cyan", |p| p.blue),
    ("base.cyan.light", |p| p.blue_soft),
];

/// Gitto's colors (see [`Palette`]) as GPUI Kit's theme roles, for `theme`; with Inter as the
/// font where it's installed (`--font-sans`), the system's otherwise.
pub fn theme_config(theme: AppTheme, inter: bool) -> ThemeConfig {
    let p = Palette::of(theme);
    let transparent = Hsla::default();
    let primary_foreground = match theme {
        AppTheme::Dark => p.primary_button_text,
        AppTheme::Light => p.avatar_text,
    };
    let mut scrollbar_thumb = p.border;
    scrollbar_thumb.a = 0.65;
    let mut selection = p.primary;
    selection.a = 0.35;
    let base: serde_json::Map<String, serde_json::Value> = BASE_COLORS
        .iter()
        .map(|(name, color)| ((*name).to_owned(), color(&p).to_hex().into()))
        .collect();
    let mut colors: ThemeConfigColors =
        serde_json::from_value(base.into()).expect("GPUI Kit's base colors are strings by name");
    set_roles!(colors {
        background: p.bg,
        foreground: p.text,
        border: p.border,
        input: p.border,
        ring: p.primary,
        caret: p.text,
        selection: selection,
        overlay: p.overlay,
        window_border: p.border,
        muted: p.panel_raised,
        muted_foreground: p.muted,
        accent: p.panel_hover,
        accent_foreground: p.text,
        popover: p.panel_raised,
        popover_foreground: p.text,
        group_box: p.panel,
        group_box_foreground: p.text,
        accordion: p.panel,
        skeleton: p.panel_raised,
        primary: p.primary,
        primary_hover: p.primary_strong,
        primary_active: p.primary_strong,
        primary_foreground: primary_foreground,
        secondary: p.panel_raised,
        secondary_hover: p.panel_hover,
        secondary_active: p.panel_active,
        secondary_foreground: p.text,
        button: p.panel_raised,
        button_hover: p.panel_hover,
        button_active: p.panel_active,
        button_foreground: p.text,
        button_primary: p.primary_button_end,
        button_primary_hover: p.primary_button_start,
        button_primary_active: p.primary_button_end,
        button_primary_foreground: p.primary_button_text,
        danger: p.coral,
        danger_hover: p.coral,
        danger_active: p.coral,
        danger_foreground: p.bg,
        warning: p.amber,
        warning_hover: p.amber,
        warning_active: p.amber,
        warning_foreground: p.bg,
        success: p.mint,
        success_hover: p.mint,
        success_active: p.mint,
        success_foreground: p.bg,
        info: p.blue,
        info_hover: p.blue,
        info_active: p.blue,
        info_foreground: p.bg,
        link: p.primary_strong,
        link_hover: p.primary,
        link_active: p.primary_strong,
        list: p.panel,
        list_even: p.panel,
        list_head: p.panel,
        list_hover: p.panel_hover,
        list_active: p.primary_soft,
        list_active_border: p.primary,
        table: p.bg,
        table_even: p.bg,
        table_head: p.bg_soft,
        table_head_foreground: p.muted,
        table_hover: p.panel_hover,
        table_active: p.primary_soft,
        table_active_border: p.primary,
        table_row_border: p.border_soft,
        sidebar: p.panel,
        sidebar_foreground: p.text,
        sidebar_border: p.border,
        sidebar_accent: p.panel_hover,
        sidebar_accent_foreground: p.text,
        sidebar_primary: p.primary_soft,
        sidebar_primary_foreground: p.primary_strong,
        tab_bar: p.header,
        tab_bar_segmented: p.bg,
        tab: transparent,
        tab_foreground: p.muted,
        tab_active: p.bg,
        tab_active_foreground: p.text,
        title_bar: p.header,
        title_bar_border: p.border,
        status_bar: p.footer,
        status_bar_border: p.border,
        switch: p.border,
        switch_thumb: p.avatar_text,
        slider_bar: p.primary,
        slider_thumb: p.avatar_text,
        progress_bar: p.primary,
        scrollbar: transparent,
        scrollbar_thumb: scrollbar_thumb,
        scrollbar_thumb_hover: p.faint,
        drag_border: p.primary,
        drop_target: p.primary_soft,
    });
    ThemeConfig {
        name: match theme {
            AppTheme::Dark => "Gitto Dark",
            AppTheme::Light => "Gitto Light",
        }
        .into(),
        mode: theme.mode(),
        font_size: Some(FONT_SIZE),
        font_family: inter.then(|| "Inter".into()),
        mono_font_size: Some(MONO_FONT_SIZE),
        radius: Some(RADIUS),
        radius_lg: Some(RADIUS_LG),
        shadow: Some(true),
        colors,
        // Without, code is drawn with GPUI Kit's light colors in either theme.
        highlight: Some(highlight_style(theme)),
        ..ThemeConfig::default()
    }
}

/// The colors of code, by the theme's highlight names (see `palette::syntax_colors`), and of the
/// code editor.
fn highlight_style(theme: AppTheme) -> HighlightThemeStyle {
    let syntax: serde_json::Map<String, serde_json::Value> = palette::syntax_colors(theme)
        .into_iter()
        .map(|(name, color)| (name.to_owned(), serde_json::json!({ "color": color })))
        .collect();
    let p = Palette::of(theme);
    HighlightThemeStyle {
        syntax: serde_json::from_value(syntax.into())
            .expect("Gitto's syntax colors are GPUI Kit's highlight names"),
        // The code editor (editing a file in its diff) on the diff's background, its line numbers
        // as the diff's.
        editor_background: Some(p.bg),
        editor_gutter_background: Some(p.bg),
        editor_line_number: Some(p.faint),
        editor_active_line_number: Some(p.text_soft),
        ..HighlightThemeStyle::default()
    }
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
        storage.set_item(STORAGE_KEY, "light");

        let theme = AppTheme::load(&storage);
        assert_eq!(theme, AppTheme::Light);
        let toggled = theme.toggled();
        storage.set_item(STORAGE_KEY, toggled.as_str());
        assert_eq!(toggled, AppTheme::Dark);
        assert_eq!(storage.get_item(STORAGE_KEY).as_deref(), Some("dark"));
    }

    #[test]
    fn falls_back_to_dark() {
        assert_eq!(AppTheme::load(&LocalStorage::in_memory()), AppTheme::Dark);
        let storage = LocalStorage::in_memory();
        storage.set_item(STORAGE_KEY, "sepia");
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
        assert_eq!(
            light.colors.tab_bar_segmented.as_deref(),
            Some(Palette::light().bg.to_hex().as_str())
        );
    }

    #[test]
    fn sets_every_base_color() {
        let colors = serde_json::to_value(theme_config(AppTheme::Dark, false).colors).unwrap();
        let dark = Palette::dark();
        for (name, color) in BASE_COLORS {
            assert_eq!(colors[name], color(&dark).to_hex(), "{name}");
        }
    }

    #[test]
    fn colors_code_by_every_syntax_color() {
        // A name GPUI Kit doesn't know would be dropped as the style is read, its code uncolored.
        for theme in [AppTheme::Dark, AppTheme::Light] {
            let highlight = theme_config(theme, false).highlight.unwrap();
            let syntax = serde_json::to_value(highlight.syntax).unwrap();
            for (name, _) in palette::syntax_colors(theme) {
                assert!(!syntax[name].is_null(), "{name}");
            }
        }
    }

    #[test]
    fn draws_the_code_editor_on_the_diffs_colors() {
        for theme in [AppTheme::Dark, AppTheme::Light] {
            let p = Palette::of(theme);
            let highlight = theme_config(theme, false).highlight.unwrap();
            assert_eq!(highlight.editor_background, Some(p.bg));
            assert_eq!(highlight.editor_gutter_background, Some(p.bg));
            assert_eq!(highlight.editor_line_number, Some(p.faint));
            assert_eq!(highlight.editor_active_line_number, Some(p.text_soft));
        }
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
    fn follows_gpui_kits_theme_mode(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| init(None, cx));

        // As GPUI Kit's own theme switcher changes it.
        cx.update(|cx| Theme::change(ThemeMode::Light, None, cx));
        assert_eq!(cx.update(|cx| cx.app_theme()), AppTheme::Light);
        assert_eq!(cx.update(|cx| *cx.palette()), Palette::light());
        cx.update(toggle);
        assert_eq!(cx.update(|cx| *cx.palette()), Palette::dark());
    }

    #[gpui_kit::test]
    fn starts_from_the_saved_theme(cx: &mut TestAppContext) {
        let storage = Rc::new(LocalStorage::in_memory());
        storage.set_item(STORAGE_KEY, "light");
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
