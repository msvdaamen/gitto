//! Gitto's colors, light and dark: `index.css`'s custom properties, by the same names. This is the
//! theme's definition, the one place raw colors belong (see the Design Guides' "Tokens before
//! values"): views read them by role, through `cx.palette()`, or through the GPUI Kit theme they're
//! mapped onto (see `super::theme_config`).

use gpui_kit::Hsla;
use gpui_kit::component::Colorize as _;

use super::AppTheme;

/// A color from a hex string in this file, which the tests check all parse.
fn hex(value: &str) -> Hsla {
    Hsla::parse_hex(value).unwrap_or_else(|_| panic!("{value} is a color"))
}

/// `rgba(r, g, b, alpha)`, as `index.css` writes the soft tints.
fn tint(value: &str, alpha: f32) -> Hsla {
    let mut color = hex(value);
    color.a = alpha;
    color
}

/// Gitto's colors in one theme. Each field is `index.css`'s variable of the same name (`--bg`,
/// `--panel-raised`…), and a few that the CSS spelled at the place it used them (gradients,
/// mixes), named for what they're for.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Palette {
    /// The window behind everything (`--bg`).
    pub bg: Hsla,
    /// Behind the header and footer, slightly off the window's (`--bg-soft`).
    pub bg_soft: Hsla,
    /// Panels and cards (`--panel`).
    pub panel: Hsla,
    /// Menus, popovers and secondary buttons, raised over a panel (`--panel-raised`).
    pub panel_raised: Hsla,
    /// What's under the pointer (`--panel-hover`).
    pub panel_hover: Hsla,
    /// What's pressed or selected (`--panel-active`).
    pub panel_active: Hsla,
    pub border: Hsla,
    /// Lines between rows in one surface (`--border-soft`).
    pub border_soft: Hsla,
    /// Body text (`--text`).
    pub text: Hsla,
    /// Secondary text, still to be read (`--text-soft`).
    pub text_soft: Hsla,
    /// Supporting text (`--muted`).
    pub muted: Hsla,
    /// The quietest text: placeholders, hints, timestamps (`--faint`).
    pub faint: Hsla,
    pub primary: Hsla,
    /// Primary text on a soft primary tint (`--primary-strong`).
    pub primary_strong: Hsla,
    pub primary_soft: Hsla,
    pub blue: Hsla,
    pub blue_soft: Hsla,
    /// Success, additions, a clean tree (`--mint`).
    pub mint: Hsla,
    pub mint_soft: Hsla,
    /// Warnings, changes (`--amber`).
    pub amber: Hsla,
    pub amber_soft: Hsla,
    /// Errors, deletions, conflicts (`--coral`).
    pub coral: Hsla,
    pub coral_soft: Hsla,
    /// The shadow under raised surfaces (`--app-shadow`'s color).
    pub shadow: Hsla,
    /// The header's background: `--bg-soft` with 8% of `--primary`.
    pub header: Hsla,
    /// The footer's background: `--bg-soft` with 6% of `--primary`.
    pub footer: Hsla,
    /// What dims the window behind a dialog.
    pub overlay: Hsla,
    /// The primary button's gradient, its border and its text, the same in both themes.
    pub primary_button_start: Hsla,
    pub primary_button_end: Hsla,
    pub primary_button_border: Hsla,
    pub primary_button_text: Hsla,
    /// Gitto's blob (the logo, the mascot): its gradient, and its face.
    pub brand_start: Hsla,
    pub brand_end: Hsla,
    pub brand_ink: Hsla,
    /// Text on an avatar, and its default color (`Avatar`'s).
    pub avatar_text: Hsla,
    pub avatar: Hsla,
}

/// Code's colors, by the theme's highlight names: the TS diff viewer's themes' (@pierre/theme's
/// `pierre-dark` and `pierre-light`), which the code in diffs and conflicts was drawn with.
pub fn syntax_colors(theme: AppTheme) -> [(&'static str, &'static str); 26] {
    let dark = theme == AppTheme::Dark;
    let pick = |dark_color, light_color| if dark { dark_color } else { light_color };
    let comment = "#737373";
    let string = pick("#5ecc71", "#199f43");
    let number = pick("#68cdf2", "#1ca1c7");
    let constant = pick("#ffd452", "#d5a910");
    let keyword = pick("#ff678d", "#d32a61");
    let variable = pick("#ffa359", "#d47628");
    let language = pick("#ffab16", "#d5901c");
    let function = pick("#9d6afb", "#693acf");
    let kind = pick("#d568ea", "#a631be");
    let punctuation = "#636363";
    let tag = pick("#ff855e", "#d5512f");
    let attribute = pick("#60d199", "#18a46c");
    [
        ("comment", comment),
        ("comment_doc", comment),
        ("string", string),
        ("string.escape", string),
        ("string.regex", string),
        ("string.special", string),
        ("text.literal", string),
        ("number", number),
        ("boolean", number),
        ("constant", constant),
        ("label", constant),
        ("keyword", keyword),
        ("preproc", keyword),
        ("variable", variable),
        ("property", variable),
        ("variable.special", language),
        ("function", function),
        ("type", kind),
        ("operator", "#08c0ef"),
        ("punctuation", punctuation),
        ("punctuation.bracket", punctuation),
        ("punctuation.delimiter", punctuation),
        ("tag", tag),
        ("title", tag),
        ("attribute", attribute),
        ("emphasis.strong", constant),
    ]
}

impl Palette {
    pub fn of(theme: AppTheme) -> Self {
        match theme {
            AppTheme::Dark => Palette::dark(),
            AppTheme::Light => Palette::light(),
        }
    }

    /// `:root` in `index.css`.
    pub fn dark() -> Self {
        let bg_soft = hex("#1a151f");
        let primary = hex("#a879df");
        Palette {
            bg: hex("#151119"),
            bg_soft,
            panel: hex("#211b27"),
            panel_raised: hex("#292130"),
            panel_hover: hex("#30273a"),
            panel_active: hex("#382c45"),
            border: hex("#3b3044"),
            border_soft: hex("#2e2635"),
            text: hex("#f2edf6"),
            text_soft: hex("#c5b9ce"),
            muted: hex("#9d91a7"),
            faint: hex("#857a8f"),
            primary,
            primary_strong: hex("#bc8cf0"),
            primary_soft: tint("#a879df", 0.15),
            blue: hex("#71c9e8"),
            blue_soft: tint("#71c9e8", 0.14),
            mint: hex("#67d4b3"),
            mint_soft: tint("#67d4b3", 0.13),
            amber: hex("#e8b75d"),
            amber_soft: tint("#e8b75d", 0.13),
            coral: hex("#ef7b80"),
            coral_soft: tint("#ef7b80", 0.12),
            shadow: tint("#040207", 0.3),
            header: bg_soft.mix_oklab(primary, 0.92),
            footer: bg_soft.mix_oklab(primary, 0.94),
            overlay: tint("#0a060e", 0.55),
            ..Palette::shared()
        }
    }

    /// `:root[data-theme="light"]` in `index.css`.
    pub fn light() -> Self {
        let bg_soft = hex("#eeebf1");
        let primary = hex("#7d4fba");
        Palette {
            bg: hex("#f5f3f7"),
            bg_soft,
            panel: hex("#ffffff"),
            panel_raised: hex("#f8f6fa"),
            panel_hover: hex("#f1edf5"),
            panel_active: hex("#eae2f1"),
            border: hex("#d8d1de"),
            border_soft: hex("#e8e3eb"),
            text: hex("#241d2a"),
            text_soft: hex("#544b5c"),
            muted: hex("#675e70"),
            faint: hex("#7d7287"),
            primary,
            primary_strong: hex("#693ca9"),
            primary_soft: tint("#7d4fba", 0.11),
            blue: hex("#2588ae"),
            blue_soft: tint("#2588ae", 0.1),
            mint: hex("#218d70"),
            mint_soft: tint("#218d70", 0.09),
            amber: hex("#a66e12"),
            amber_soft: tint("#a66e12", 0.1),
            coral: hex("#c24f58"),
            coral_soft: tint("#c24f58", 0.09),
            shadow: tint("#2e213a", 0.12),
            header: bg_soft.mix_oklab(primary, 0.92),
            footer: bg_soft.mix_oklab(primary, 0.94),
            overlay: tint("#0a060e", 0.35),
            ..Palette::shared()
        }
    }

    /// What both themes share, filled in by each.
    fn shared() -> Self {
        let none = Hsla::default();
        Palette {
            bg: none,
            bg_soft: none,
            panel: none,
            panel_raised: none,
            panel_hover: none,
            panel_active: none,
            border: none,
            border_soft: none,
            text: none,
            text_soft: none,
            muted: none,
            faint: none,
            primary: none,
            primary_strong: none,
            primary_soft: none,
            blue: none,
            blue_soft: none,
            mint: none,
            mint_soft: none,
            amber: none,
            amber_soft: none,
            coral: none,
            coral_soft: none,
            shadow: none,
            header: none,
            footer: none,
            overlay: none,
            primary_button_start: hex("#c696ed"),
            primary_button_end: hex("#9f72db"),
            primary_button_border: hex("#ad7ada"),
            primary_button_text: hex("#1f1328"),
            brand_start: hex("#bc8aef"),
            brand_end: hex("#7861e6"),
            brand_ink: hex("#261c31"),
            avatar_text: hex("#ffffff"),
            avatar: hex("#8c65cf"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_color_parses() {
        for theme in [AppTheme::Dark, AppTheme::Light] {
            let palette = Palette::of(theme);
            assert_ne!(palette.bg, Hsla::default());
            assert!(palette.primary_soft.a < 0.2);
        }
    }

    #[test]
    fn the_themes_differ() {
        assert_ne!(Palette::dark().bg, Palette::light().bg);
        assert_eq!(
            Palette::dark().primary_button_start,
            Palette::light().primary_button_start
        );
    }
}
