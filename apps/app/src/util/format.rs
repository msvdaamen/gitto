//! Formatting for display, from `lib/format.ts` (gitto-ui's `format`), and what else the app
//! formats.

pub use gitto_ui::format::{compact_count, initials, relative_time};
use gpui_kit::Hsla;

use crate::theme::palette;

/// An avatar colour picked by `key` (e.g. an email address), the same one every time: gitto-ui's
/// pick, from the palette's avatar colours (the same ones).
pub fn avatar_color(key: &str) -> Hsla {
    let picked = gitto_ui::format::avatar_color(key);
    let index = palette::AVATAR_COLORS
        .iter()
        .position(|color| color.eq_ignore_ascii_case(picked))
        .unwrap_or(0);
    palette::avatar_color(index)
}

/// `text` with its first letter upper case (`capitalize` in `@gitto/release/changelog`).
pub fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// A YYYY-MM-DD date as English short dates go, e.g. `Oct 12, 2026`; as it is if it isn't one
/// (`formatDate` in `whats-new-dialog.tsx`).
pub fn short_date(date: &str) -> String {
    let parts: Vec<&str> = date.split('-').collect();
    let parsed = match parts.as_slice() {
        [year, month, day] if year.len() == 4 && month.len() == 2 && day.len() == 2 => {
            match (
                year.parse::<u32>(),
                month.parse::<usize>(),
                day.parse::<u32>(),
            ) {
                (Ok(year), Ok(month @ 1..=12), Ok(day @ 1..=31)) => Some((year, month, day)),
                _ => None,
            }
        }
        _ => None,
    };
    match parsed {
        Some((year, month, day)) => format!("{} {day}, {year}", MONTHS[month - 1]),
        None => date.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_gitto_uis_avatar_color() {
        for key in ["ada@example.com", "grace@example.com", "linus@example.com"] {
            let index = palette::AVATAR_COLORS
                .iter()
                .position(|color| *color == gitto_ui::format::avatar_color(key));
            assert_eq!(index.map(palette::avatar_color), Some(avatar_color(key)));
        }
    }

    #[test]
    fn picks_the_same_avatar_color_every_time() {
        assert_eq!(
            avatar_color("ada@example.com"),
            avatar_color("ADA@example.com")
        );
    }

    #[test]
    fn says_a_date_as_a_short_date() {
        assert_eq!(short_date("2026-10-12"), "Oct 12, 2026");
        assert_eq!(short_date("someday"), "someday");
        assert_eq!(capitalize("add worktrees"), "Add worktrees");
    }
}
