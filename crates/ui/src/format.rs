//! Ported from `lib/format.ts`.

/// Up to two letters for an avatar: `AL` for "Ada Lovelace", `AD` for "ada".
pub fn initials(name: &str) -> String {
    let mut words = name
        .split(|c: char| c.is_whitespace() || matches!(c, '.' | '_' | '-'))
        .filter(|word| !word.is_empty());
    let first = words.next();
    let last = words.next_back();
    let letters: String = match (first, last) {
        (Some(first), Some(last)) => first.chars().take(1).chain(last.chars().take(1)).collect(),
        _ => name.chars().take(2).collect(),
    };
    letters.to_uppercase()
}

const AVATAR_COLORS: [&str; 6] = [
    "#7c5ce7", "#38bda9", "#a978dd", "#e2a646", "#5b9be6", "#e0707a",
];

/// An avatar colour picked by `key` (e.g. an email address), whatever its case, the same one
/// every time: by the 32-bit FNV-1a hash of its lowercase UTF-8.
pub fn avatar_color(key: &str) -> &'static str {
    let mut hash: u32 = 0x811c_9dc5;
    let mut utf8 = [0; 4];
    for c in key.chars().flat_map(char::to_lowercase) {
        for byte in c.encode_utf8(&mut utf8).bytes() {
            hash = (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193);
        }
    }
    AVATAR_COLORS[hash as usize % AVATAR_COLORS.len()]
}

const MINUTE: i64 = 60 * 1000;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;

/// The units relative times are told in, largest first, with how long each is in milliseconds.
const TIME_UNITS: [(TimeUnit, i64); 6] = [
    (TimeUnit::Year, 365 * DAY),
    (TimeUnit::Month, 30 * DAY),
    (TimeUnit::Week, 7 * DAY),
    (TimeUnit::Day, DAY),
    (TimeUnit::Hour, HOUR),
    (TimeUnit::Minute, MINUTE),
];

#[derive(Clone, Copy)]
enum TimeUnit {
    Year,
    Month,
    Week,
    Day,
    Hour,
    Minute,
}

impl TimeUnit {
    fn name(self) -> &'static str {
        match self {
            TimeUnit::Year => "year",
            TimeUnit::Month => "month",
            TimeUnit::Week => "week",
            TimeUnit::Day => "day",
            TimeUnit::Hour => "hour",
            TimeUnit::Minute => "minute",
        }
    }
}

/// `timestamp` (milliseconds since the epoch) relative to `now`, e.g. "3 days ago".
///
/// Worded as `Intl.RelativeTimeFormat` words it in English with `numeric: "auto"`: "yesterday",
/// "last week", "in 2 hours". (The Electron app used the system's language.)
pub fn relative_time(timestamp: i64, now: i64) -> String {
    let diff = timestamp - now;
    for (unit, size) in TIME_UNITS {
        if diff.abs() >= size {
            return format_relative((diff as f64 / size as f64).round() as i64, unit);
        }
    }
    "Just now".to_owned()
}

/// `Intl.RelativeTimeFormat("en", { numeric: "auto" }).format(value, unit)`, for a `value` that
/// isn't 0.
fn format_relative(value: i64, unit: TimeUnit) -> String {
    match (value, unit) {
        (-1, TimeUnit::Day) => "yesterday".to_owned(),
        (1, TimeUnit::Day) => "tomorrow".to_owned(),
        (-1, TimeUnit::Week | TimeUnit::Month | TimeUnit::Year) => format!("last {}", unit.name()),
        (1, TimeUnit::Week | TimeUnit::Month | TimeUnit::Year) => format!("next {}", unit.name()),
        _ => {
            let count = value.unsigned_abs();
            let plural = if count == 1 { "" } else { "s" };
            let amount = format!("{} {}{plural}", group_thousands(count), unit.name());
            if value < 0 {
                format!("{amount} ago")
            } else {
                format!("in {amount}")
            }
        }
    }
}

/// `count` with its thousands grouped, as English writes it: `12,345`.
fn group_thousands(count: u64) -> String {
    let digits = count.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

/// A count short enough for a badge: `1.2k` for 1234.
pub fn compact_count(count: u64) -> String {
    if count < 1000 {
        return count.to_string();
    }
    if count >= 10_000 {
        return format!("{}k", (count + 500) / 1000);
    }
    // Under 10k, to a tenth of a thousand, rounded halves up.
    let tenths = (count + 50) / 100;
    match tenths % 10 {
        0 => format!("{}k", tenths / 10),
        tenth => format!("{}.{tenth}k", tenths / 10),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod initials {
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
    }

    mod relative_time {
        use super::*;

        /// `Date.UTC(2026, 0, 31)`.
        const NOW: i64 = 1_769_817_600_000;

        #[test]
        fn uses_the_largest_unit_that_fits() {
            assert_eq!(
                relative_time(NOW - 3 * 24 * 60 * 60 * 1000, NOW),
                "3 days ago"
            );
            assert_eq!(relative_time(NOW - 2 * 60 * 60 * 1000, NOW), "2 hours ago");
        }

        #[test]
        fn says_just_now_for_less_than_a_minute() {
            assert_eq!(relative_time(NOW - 30_000, NOW), "Just now");
        }

        #[test]
        fn words_it_as_intl_does_in_english() {
            assert_eq!(relative_time(NOW - DAY, NOW), "yesterday");
            assert_eq!(relative_time(NOW - 8 * DAY, NOW), "last week");
            assert_eq!(relative_time(NOW + 2 * HOUR, NOW), "in 2 hours");
            assert_eq!(relative_time(NOW - MINUTE, NOW), "1 minute ago");
            assert_eq!(
                relative_time(NOW - 1500 * 365 * DAY, NOW),
                "1,500 years ago"
            );
        }
    }

    mod compact_count {
        use super::*;

        #[test]
        fn shortens_thousands() {
            assert_eq!(compact_count(999), "999");
            assert_eq!(compact_count(1234), "1.2k");
            assert_eq!(compact_count(2000), "2k");
            assert_eq!(compact_count(12_345), "12k");
        }
    }

    #[test]
    fn picks_avatar_colors_by_the_fnv_1a_hash_of_the_lowercase_key() {
        // FNV-1a: "" hashes to 0x811c9dc5, "a" to 0xe40c292c.
        assert_eq!(avatar_color(""), AVATAR_COLORS[0x811c_9dc5 % 6]);
        assert_eq!(avatar_color("A"), AVATAR_COLORS[0xe40c_292c % 6]);
        assert_eq!(
            avatar_color("ada@example.com"),
            avatar_color("Ada@Example.com")
        );
        assert_eq!(
            avatar_color("ÉMILE@example.com"),
            avatar_color("émile@example.com")
        );
    }
}
