//! Ported from `lib/format.ts`.

use std::cell::RefCell;
use std::rc::Rc;

use fixed_decimal::Decimal;
use icu_experimental::relativetime::options::Numeric;
use icu_experimental::relativetime::{RelativeTimeFormatter, RelativeTimeFormatterOptions};
use icu_locale_core::{Locale, locale};

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

/// The avatar colours, as `0xRRGGBB`.
pub const AVATAR_COLORS: [u32; 6] = [0x7c5ce7, 0x38bda9, 0xa978dd, 0xe2a646, 0x5b9be6, 0xe0707a];

/// An avatar colour, as `0xRRGGBB`, picked by `key` (e.g. an email address), whatever its case,
/// the same one every time, and the one the Electron app picked: by TS's hash of its lowercase
/// text, `hash * 31 + c` over each character's first UTF-16 unit, in 32 bits.
pub fn avatar_color(key: &str) -> u32 {
    let mut hash: i32 = 0;
    let mut utf16 = [0; 2];
    for c in key.to_lowercase().chars() {
        let unit = c.encode_utf16(&mut utf16)[0];
        hash = hash.wrapping_mul(31).wrapping_add(i32::from(unit));
    }
    AVATAR_COLORS[hash.unsigned_abs() as usize % AVATAR_COLORS.len()]
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
    /// ICU4X's formatter for the unit in `locale`, as `Intl.RelativeTimeFormat` words it with
    /// `numeric: "auto"`: "yesterday", "last week", "in 2 hours".
    fn formatter(self, locale: &Locale) -> Option<RelativeTimeFormatter> {
        let prefs = locale.into();
        let options = RelativeTimeFormatterOptions::default().with_numeric(Numeric::Auto);
        let formatter = match self {
            TimeUnit::Year => RelativeTimeFormatter::try_new_long_year(prefs, options),
            TimeUnit::Month => RelativeTimeFormatter::try_new_long_month(prefs, options),
            TimeUnit::Week => RelativeTimeFormatter::try_new_long_week(prefs, options),
            TimeUnit::Day => RelativeTimeFormatter::try_new_long_day(prefs, options),
            TimeUnit::Hour => RelativeTimeFormatter::try_new_long_hour(prefs, options),
            TimeUnit::Minute => RelativeTimeFormatter::try_new_long_minute(prefs, options),
        };
        formatter.ok()
    }
}

/// Relative times told in a language, as `Intl.RelativeTimeFormat` tells them: the number in it
/// too, with its thousands grouped as it groups them ("1,500 years ago", "1.500 jaar geleden").
pub struct RelativeTimes {
    /// By [`TIME_UNITS`]' order.
    formatters: [RelativeTimeFormatter; TIME_UNITS.len()],
}

impl RelativeTimes {
    /// Times told in `language`, a BCP 47 tag like `nl-NL`; in English if it isn't one, or there's
    /// no wording for it.
    pub fn new(language: &str) -> Self {
        let locale = Locale::try_from_str(language).unwrap_or(locale!("en"));
        let formatters = |locale: &Locale| {
            let [year, month, week, day, hour, minute] =
                TIME_UNITS.map(|(unit, _)| unit.formatter(locale));
            Some([year?, month?, week?, day?, hour?, minute?])
        };
        let formatters = formatters(&locale)
            .or_else(|| formatters(&locale!("en")))
            .expect("ICU4X's compiled data words relative times in English");
        RelativeTimes { formatters }
    }

    /// In the system's language, as the Electron app's renderer told them.
    pub fn system() -> Self {
        RelativeTimes::new(&sys_locale::get_locale().unwrap_or_default())
    }

    /// `timestamp` (milliseconds since the epoch) relative to `now`, e.g. "3 days ago".
    pub fn format(&self, timestamp: i64, now: i64) -> String {
        let diff = timestamp - now;
        for ((_, size), formatter) in TIME_UNITS.iter().zip(&self.formatters) {
            if diff.abs() >= *size {
                let value = round_half_up(diff as f64 / *size as f64);
                return formatter.format(Decimal::from(value)).to_string();
            }
        }
        // As TS had it, whatever the language.
        "Just now".to_owned()
    }
}

thread_local! {
    /// What [`relative_time`] tells times with: made on first use, on the thread that uses it
    /// (the UI's), as they aren't `Send`.
    static RELATIVE_TIMES: RefCell<Option<Rc<RelativeTimes>>> = const { RefCell::new(None) };
}

/// `timestamp` (milliseconds since the epoch) relative to `now`, e.g. "3 days ago", in the system's
/// language (see [`RelativeTimes`]), or the one [`tell_relative_times_in`] set.
pub fn relative_time(timestamp: i64, now: i64) -> String {
    let times = RELATIVE_TIMES.with_borrow_mut(|times| {
        Rc::clone(times.get_or_insert_with(|| Rc::new(RelativeTimes::system())))
    });
    times.format(timestamp, now)
}

/// Has [`relative_time`] tell times in `language` (see [`RelativeTimes::new`]) on this thread,
/// rather than the system's: e.g. in a test, in English whatever the system's is.
pub fn tell_relative_times_in(language: &str) {
    let times = Rc::new(RelativeTimes::new(language));
    RELATIVE_TIMES.with_borrow_mut(|current| *current = Some(times));
}

/// `x` rounded as JS's `Math.round` rounds it: halves up, so -1.5 is -1 (`f64::round` makes it
/// -2).
fn round_half_up(x: f64) -> i64 {
    let floor = x.floor();
    if x - floor >= 0.5 {
        floor as i64 + 1
    } else {
        floor as i64
    }
}

/// A count short enough for a badge: `1.2k` for 1234.
pub fn compact_count(count: u64) -> String {
    if count < 1000 {
        return count.to_string();
    }
    if count >= 10_000 {
        // TS's `toFixed(0)`: a half (10.5) is exact, and rounds up.
        return format!("{}k", (count + 500) / 1000);
    }
    // Under 10k, to a tenth of a thousand, as TS's `toFixed(1)` rounds `count / 1000`: halves up,
    // but it rounds the nearest double, which for some halves (1.15, 9.95) is just under them.
    // (`mul_add` takes the count from that double times 1000 exactly, so the sign is right.)
    let thousands = count as f64 / 1000.;
    let under = thousands.mul_add(1000., -(count as f64)) < 0.;
    let tenths = if count % 100 == 50 && under {
        count / 100
    } else {
        (count + 50) / 100
    };
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

        fn english(timestamp: i64) -> String {
            RelativeTimes::new("en").format(timestamp, NOW)
        }

        #[test]
        fn uses_the_largest_unit_that_fits() {
            assert_eq!(english(NOW - 3 * 24 * 60 * 60 * 1000), "3 days ago");
            assert_eq!(english(NOW - 2 * 60 * 60 * 1000), "2 hours ago");
        }

        #[test]
        fn says_just_now_for_less_than_a_minute() {
            assert_eq!(english(NOW - 30_000), "Just now");
            assert_eq!(
                RelativeTimes::new("nl").format(NOW - 30_000, NOW),
                "Just now"
            );
        }

        #[test]
        fn words_it_as_intl_does_in_english() {
            assert_eq!(english(NOW - DAY), "yesterday");
            assert_eq!(english(NOW + DAY), "tomorrow");
            assert_eq!(english(NOW - 8 * DAY), "last week");
            assert_eq!(english(NOW - 45 * DAY), "last month");
            assert_eq!(english(NOW + 2 * HOUR), "in 2 hours");
            assert_eq!(english(NOW - MINUTE), "1 minute ago");
            assert_eq!(english(NOW - 1500 * 365 * DAY), "1,500 years ago");
        }

        #[test]
        fn rounds_halves_up_as_math_round_does() {
            // What TS told these times as.
            assert_eq!(english(NOW - 3 * MINUTE / 2), "1 minute ago");
            assert_eq!(english(NOW + 3 * MINUTE / 2), "in 2 minutes");
            assert_eq!(english(NOW - 36 * HOUR), "yesterday");
            assert_eq!(english(NOW - 5 * HOUR / 2), "2 hours ago");
        }

        #[test]
        fn words_it_as_intl_does_in_other_languages() {
            // What `Intl.RelativeTimeFormat("nl" or "de", { numeric: "auto" })` says.
            let dutch = RelativeTimes::new("nl-NL");
            assert_eq!(dutch.format(NOW - 3 * DAY, NOW), "3 dagen geleden");
            assert_eq!(dutch.format(NOW - DAY, NOW), "gisteren");
            assert_eq!(dutch.format(NOW + 2 * HOUR, NOW), "over 2 uur");
            assert_eq!(
                dutch.format(NOW - 1500 * 365 * DAY, NOW),
                "1.500 jaar geleden"
            );
            let german = RelativeTimes::new("de");
            assert_eq!(german.format(NOW - 8 * DAY, NOW), "letzte Woche");
            assert_eq!(german.format(NOW - 3 * MINUTE / 2, NOW), "vor 1 Minute");
            assert_eq!(
                german.format(NOW - 1500 * 365 * DAY, NOW),
                "vor 1.500 Jahren"
            );
        }

        #[test]
        fn words_it_in_english_for_what_isnt_a_language() {
            assert_eq!(RelativeTimes::new("C").format(NOW - DAY, NOW), "yesterday");
            assert_eq!(RelativeTimes::new("").format(NOW - DAY, NOW), "yesterday");
        }

        #[test]
        fn tells_times_in_the_language_set() {
            tell_relative_times_in("nl");
            assert_eq!(relative_time(NOW - DAY, NOW), "gisteren");
            tell_relative_times_in("en");
            assert_eq!(relative_time(NOW - DAY, NOW), "yesterday");
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

        #[test]
        fn rounds_as_to_fixed_does() {
            // What TS's `compactCount` gave.
            for (count, compact) in [
                (1050, "1.1k"),
                (1150, "1.1k"),
                (1250, "1.3k"),
                (1450, "1.4k"),
                (1950, "1.9k"),
                (2050, "2k"),
                (9850, "9.8k"),
                (9949, "9.9k"),
                (9950, "9.9k"),
                (9951, "10k"),
                (9999, "10k"),
                (10_499, "10k"),
                (10_500, "11k"),
            ] {
                assert_eq!(compact_count(count), compact, "{count}");
            }
        }
    }

    #[test]
    fn picks_avatar_colors_as_ts_did() {
        // What TS's `avatarColor` picked.
        for (key, color) in [
            ("", 0x7c5ce7),
            ("a", 0x38bda9),
            ("ada@example.com", 0xe2a646),
            ("Ada@Example.com", 0xe2a646),
            ("ÉMILE@example.com", 0xe0707a),
            ("linus@kernel.org", 0x5b9be6),
            ("🦀rust@example.com", 0xe0707a),
            ("ΟΔΟΣ", 0x7c5ce7),
            (
                "a-very-long-email-address-to-overflow@example.com",
                0xe2a646,
            ),
        ] {
            assert_eq!(avatar_color(key), color, "{key}");
        }
    }
}
