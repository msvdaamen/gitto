//! The accent colors used for badges, chips and counts. Ported from `components/ui/tone.ts`.

use gpui_kit::Hsla;

use crate::theme::Palette;

/// An accent color, by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Tone {
    #[default]
    Neutral,
    Purple,
    Blue,
    Mint,
    Amber,
    Coral,
}

impl Tone {
    /// A soft background with matching text, for small colored chips (`toneClasses`).
    pub fn colors(self, palette: &Palette) -> (Hsla, Hsla) {
        match self {
            Tone::Neutral => (palette.panel_raised, palette.muted),
            Tone::Purple => (palette.primary_soft, palette.primary_strong),
            Tone::Blue => (palette.blue_soft, palette.blue),
            Tone::Mint => (palette.mint_soft, palette.mint),
            Tone::Amber => (palette.amber_soft, palette.amber),
            Tone::Coral => (palette.coral_soft, palette.coral),
        }
    }

    /// A badge's border: the tone at 30%, or the border for neutral and mint (`badge.tsx`).
    pub fn border(self, palette: &Palette) -> Hsla {
        let tinted = |color: Hsla| {
            let mut color = color;
            color.a *= 0.3;
            color
        };
        match self {
            Tone::Neutral | Tone::Mint => palette.border,
            Tone::Purple => tinted(palette.primary),
            Tone::Blue => tinted(palette.blue),
            Tone::Amber => tinted(palette.amber),
            Tone::Coral => tinted(palette.coral),
        }
    }
}
