//! A hunk's header in a patch, `@@ -a,b +c,d @@ heading`: read here for staging lines (see
//! `staging::lines`) and by the app's diff viewer alike, so the lines a selection picks are counted
//! the same way by both.

/// The numbers of a hunk's `@@ -a,b +c,d @@` line, and what follows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HunkHeader<'a> {
    /// The first line of the old file the hunk covers, and how many lines it covers there.
    pub old_start: usize,
    pub old_count: usize,
    /// The same, of the new file.
    pub new_start: usize,
    pub new_count: usize,
    /// What follows the second `@@`, as it is, without the line end: after a space, what git says
    /// the lines are in, like a function's name.
    pub heading: &'a str,
}

impl<'a> HunkHeader<'a> {
    /// What git says the lines are in, if it says: the heading, after its space.
    pub fn context(&self) -> Option<&'a str> {
        self.heading.strip_prefix(' ')
    }
}

/// Reads `line` as a hunk's header, with its line end (LF or CRLF) or without; `None` if it isn't
/// one. A count left out is 1, as git leaves out a count of 1; numbers are ASCII digits only.
pub fn parse_hunk_header(line: &str) -> Option<HunkHeader<'_>> {
    let line = line.strip_suffix('\n').unwrap_or(line);
    let line = line.strip_suffix('\r').unwrap_or(line);
    let rest = line.strip_prefix("@@ -")?;
    let (old_start, old_count, rest) = read_range(rest)?;
    let rest = rest.strip_prefix(" +")?;
    let (new_start, new_count, rest) = read_range(rest)?;
    let heading = rest.strip_prefix(" @@")?;
    Some(HunkHeader {
        old_start,
        old_count,
        new_start,
        new_count,
        heading,
    })
}

/// A side's `start,count` (or `start` alone, of one line), and what follows it.
fn read_range(text: &str) -> Option<(usize, usize, &str)> {
    let (start, rest) = read_number(text)?;
    match rest.strip_prefix(',') {
        Some(rest) => {
            let (count, rest) = read_number(rest)?;
            Some((start, count, rest))
        }
        None => Some((start, 1, rest)),
    }
}

fn read_number(text: &str) -> Option<(usize, &str)> {
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let value = text[..digits].parse().ok()?;
    Some((value, &text[digits..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(
        (old_start, old_count): (usize, usize),
        (new_start, new_count): (usize, usize),
        heading: &str,
    ) -> Option<HunkHeader<'_>> {
        Some(HunkHeader {
            old_start,
            old_count,
            new_start,
            new_count,
            heading,
        })
    }

    #[test]
    fn reads_both_sides_and_the_heading() {
        assert_eq!(
            parse_hunk_header("@@ -1,5 +2,6 @@ fn main()"),
            header((1, 5), (2, 6), " fn main()")
        );
        assert_eq!(
            parse_hunk_header("@@ -1,5 +2,6 @@ fn main()").and_then(|header| header.context()),
            Some("fn main()")
        );
        assert_eq!(
            parse_hunk_header("@@ -0,0 +1,2 @@"),
            header((0, 0), (1, 2), "")
        );
        assert_eq!(
            parse_hunk_header("@@ -0,0 +1,2 @@").unwrap().context(),
            None
        );
    }

    #[test]
    fn takes_a_count_left_out_as_one() {
        assert_eq!(parse_hunk_header("@@ -3 +3 @@"), header((3, 1), (3, 1), ""));
        assert_eq!(
            parse_hunk_header("@@ -1 +0,0 @@"),
            header((1, 1), (0, 0), "")
        );
    }

    #[test]
    fn leaves_the_line_end_out() {
        let expected = header((1, 2), (1, 3), " heading");
        assert_eq!(parse_hunk_header("@@ -1,2 +1,3 @@ heading\n"), expected);
        assert_eq!(parse_hunk_header("@@ -1,2 +1,3 @@ heading\r\n"), expected);
        assert_eq!(parse_hunk_header("@@ -1,2 +1,3 @@ heading\r"), expected);
    }

    #[test]
    fn is_none_for_what_isnt_a_header() {
        for line in [
            "",
            "@@",
            "@@ -1 +1",
            "@@ -1 +1 @",
            "@@ -a,1 +1 @@",
            "@@ -1, +1 @@",
            "@@ -1,1 -1,1 @@",
            "@@  -1 +1 @@",
            "@@ -١ +1 @@",
            " @@ -1 +1 @@",
            "@@ -99999999999999999999999 +1 @@",
        ] {
            assert_eq!(parse_hunk_header(line), None, "{line:?}");
        }
    }
}
