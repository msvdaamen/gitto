//! What JavaScript does that the ported code relied on, where Rust's own does it differently.

/// Whether `c` is whitespace to JavaScript: what `\s` matches, and `trim()` drops.
pub(crate) fn is_whitespace(c: char) -> bool {
    const SPACES: [char; 9] = [
        ' ', '\u{a0}', '\u{1680}', '\u{2028}', '\u{2029}', '\u{202f}', '\u{205f}', '\u{3000}',
        '\u{feff}',
    ];
    // `\t`, `\n`, `\v`, `\f` and `\r`; and the spaces from the en quad to the hair space.
    ('\t'..='\r').contains(&c) || ('\u{2000}'..='\u{200a}').contains(&c) || SPACES.contains(&c)
}

/// `text.trimEnd()`.
pub(crate) fn trim_end(text: &str) -> &str {
    text.trim_end_matches(is_whitespace)
}

/// `Math.round(x)`: halves round up, towards positive infinity.
pub(crate) fn round(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

/// `x.toFixed(digits)` for a finite `x` of at least 0: rounded from the exact value of the float,
/// halves up, as JavaScript does (Rust's own formatting rounds halves to even).
pub(crate) fn to_fixed(x: f64, digits: usize) -> String {
    // A float under 2^53 has at most 1074 digits after the point, but one this function is given
    // (a count of thousands) has far fewer: 64 are exact for every float of 1 or more.
    let exact = format!("{x:.80}");
    let (whole, fraction) = exact.split_once('.').unwrap_or((&exact, ""));
    let mut digits_kept: Vec<u8> = whole.bytes().chain(fraction.bytes().take(digits)).collect();
    let rounds_up = fraction
        .as_bytes()
        .get(digits)
        .is_some_and(|&next| next >= b'5');
    if rounds_up {
        let mut index = digits_kept.len();
        loop {
            if index == 0 {
                digits_kept.insert(0, b'1');
                break;
            }
            index -= 1;
            if digits_kept[index] == b'9' {
                digits_kept[index] = b'0';
            } else {
                digits_kept[index] += 1;
                break;
            }
        }
    }
    let point = digits_kept.len() - digits;
    let mut fixed = String::from_utf8(digits_kept).expect("ASCII digits");
    if digits > 0 {
        fixed.insert(point, '.');
    }
    fixed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_fixed_rounds_the_exact_value_halves_up() {
        assert_eq!(to_fixed(1.25, 1), "1.3");
        assert_eq!(to_fixed(1.15, 1), "1.1");
        assert_eq!(to_fixed(9.96, 1), "10.0");
        assert_eq!(to_fixed(12.5, 0), "13");
        assert_eq!(to_fixed(12.345, 0), "12");
        assert_eq!(to_fixed(99.5, 0), "100");
    }

    #[test]
    fn round_rounds_halves_up() {
        assert_eq!(round(2.5), 3.0);
        assert_eq!(round(-2.5), -2.0);
        assert_eq!(round(0.499_999_999_999_999_94), 0.0);
    }
}
