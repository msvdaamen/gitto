//! Formatting for display. Ported from `lib/format.ts`.

use gpui_kit::Hsla;

use crate::theme::palette;

/// Up to two letters for an avatar: `AL` for "Ada Lovelace", `AD` for "ada".
pub fn initials(name: &str) -> String {
    let words: Vec<&str> = name
        .split(|c: char| c.is_whitespace() || matches!(c, '.' | '_' | '-'))
        .filter(|word| !word.is_empty())
        .collect();
    let letters: String = if words.len() > 1 {
        words[0]
            .chars()
            .take(1)
            .chain(words[words.len() - 1].chars().take(1))
            .collect()
    } else {
        name.chars().take(2).collect()
    };
    letters.to_uppercase()
}

/// An avatar colour picked by `key` (e.g. an email address), the same one every time.
pub fn avatar_color(key: &str) -> Hsla {
    let mut hash: i32 = 0;
    for unit in key.to_lowercase().encode_utf16() {
        hash = hash.wrapping_mul(31).wrapping_add(i32::from(unit));
    }
    palette::avatar_color(hash.unsigned_abs() as usize)
}

const MINUTE: i64 = 60 * 1000;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;
const WEEK: i64 = 7 * DAY;
const MONTH: i64 = 30 * DAY;
const YEAR: i64 = 365 * DAY;

const TIME_UNITS: [(&str, i64); 6] = [
    ("year", YEAR),
    ("month", MONTH),
    ("week", WEEK),
    ("day", DAY),
    ("hour", HOUR),
    ("minute", MINUTE),
];

/// `timestamp` (milliseconds since the epoch) relative to `now`, e.g. "3 days ago": in the largest
/// unit that fits, as `Intl.RelativeTimeFormat` says it in English with `numeric: "auto"`
/// ("yesterday", "last week").
pub fn relative_time(timestamp: i64, now: i64) -> String {
    let diff = timestamp - now;
    for (unit, size) in TIME_UNITS {
        if diff.abs() >= size {
            let count = (diff as f64 / size as f64).round() as i64;
            return relative(count, unit);
        }
    }
    "Just now".to_owned()
}

fn relative(count: i64, unit: &str) -> String {
    match (count, unit) {
        (-1, "day") => "yesterday".to_owned(),
        (1, "day") => "tomorrow".to_owned(),
        (-1, "week" | "month" | "year") => format!("last {unit}"),
        (1, "week" | "month" | "year") => format!("next {unit}"),
        (count, unit) => {
            let plural = if count.abs() == 1 { "" } else { "s" };
            if count < 0 {
                format!("{} {unit}{plural} ago", -count)
            } else {
                format!("in {count} {unit}{plural}")
            }
        }
    }
}

/// A count short enough for a badge: `1.2k` for 1234.
pub fn compact_count(count: u64) -> String {
    if count < 1000 {
        return count.to_string();
    }
    let thousands = count as f64 / 1000.;
    let text = if count < 10_000 {
        format!("{thousands:.1}")
    } else {
        format!("{thousands:.0}")
    };
    format!("{}k", text.strip_suffix(".0").unwrap_or(&text))
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
    fn takes_the_first_and_last_words_first_letters() {
        assert_eq!(initials("Ada King Lovelace"), "AL");
        assert_eq!(initials("ada.lovelace"), "AL");
    }

    #[test]
    fn takes_the_first_two_letters_of_a_single_word() {
        assert_eq!(initials("ada"), "AD");
    }

    const NOW: i64 = 1_769_817_600_000; // 2026-01-31

    #[test]
    fn uses_the_largest_unit_that_fits() {
        assert_eq!(relative_time(NOW - 3 * DAY, NOW), "3 days ago");
        assert_eq!(relative_time(NOW - 2 * HOUR, NOW), "2 hours ago");
        assert_eq!(relative_time(NOW - MINUTE, NOW), "1 minute ago");
        assert_eq!(relative_time(NOW - DAY, NOW), "yesterday");
    }

    #[test]
    fn says_just_now_for_less_than_a_minute() {
        assert_eq!(relative_time(NOW - 30_000, NOW), "Just now");
    }

    #[test]
    fn shortens_thousands() {
        assert_eq!(compact_count(999), "999");
        assert_eq!(compact_count(1234), "1.2k");
        assert_eq!(compact_count(2000), "2k");
        assert_eq!(compact_count(12_345), "12k");
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
