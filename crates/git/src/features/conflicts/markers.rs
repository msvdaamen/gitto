//! Conflict markers, as git leaves them in a file it couldn't merge: `<<<<<<< ours`, then the
//! base's lines after `||||||| base` with the diff3 and zdiff3 styles, then `=======`, then
//! `>>>>>>> theirs`. Ported from `markers.ts`.
//!
//! The text is read as bytes, each one a character as Latin-1 has it (TS read the file that way):
//! markers are ASCII, which reads the same in any of the encodings git merges as text.

/// How many bytes from its start git looks at to tell whether a file is binary: for a NUL.
pub const BINARY_CHECK_BYTES: usize = 8000;

/// Whether git takes `bytes` for a binary file's, as it does when it merges: not text to mark.
pub fn is_binary(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(BINARY_CHECK_BYTES)].contains(&0)
}

/// How many conflicts `text` still has: regions from a start marker to an end marker, with a
/// separator between. A marker alone isn't one, nor are markers of another length than 7 or more
/// (git's own, unless `conflict-marker-size` says otherwise), so a file that only mentions them, or
/// a heading underlined with `=`, has none. Conflicts inside conflicts, which a recursive merge can
/// leave, count once each.
pub fn count_conflicts(text: &str) -> usize {
    let mut counter = ConflictCounter::new();
    counter.push(text.as_bytes());
    counter.end()
}

/// Past this many characters, a line's rest isn't kept while the next piece of it comes: a marker
/// is told by its start, and a separator line this long isn't one.
const KEPT_LINE: usize = 4096;

/// Counts conflicts (see `count_conflicts`) in text given a piece at a time, as a file is read: one
/// of hundreds of megabytes is never all in memory.
#[derive(Debug, Default)]
pub struct ConflictCounter {
    count: usize,
    /// Whether each open conflict, innermost last, has had its separator.
    open: Vec<bool>,
    /// The start of a line whose end hasn't come yet.
    rest: Vec<u8>,
}

impl ConflictCounter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Counts the conflicts in the next piece of the text.
    pub fn push(&mut self, piece: &[u8]) {
        let mut text = std::mem::take(&mut self.rest);
        text.extend_from_slice(piece);
        let Some(last) = memchr::memrchr(b'\n', &text) else {
            // Too long to be a marker's line but for its start; and no separator, with this after
            // it.
            if text.len() > KEPT_LINE {
                text.truncate(KEPT_LINE);
                text.push(b'x');
            }
            self.rest = text;
            return;
        };
        self.scan(&text, last);
        let from = last + 1;
        let to = (from + KEPT_LINE + 1).min(text.len());
        self.rest = text[from..to].to_vec();
    }

    /// The conflicts in all of the text given, which has ended.
    pub fn end(&mut self) -> usize {
        if !self.rest.is_empty() {
            let rest = std::mem::take(&mut self.rest);
            self.scan(&rest, rest.len());
        }
        self.count
    }

    /// Reads the lines of `text` up to `to`, the newline of its last whole one, or its end.
    fn scan(&mut self, text: &[u8], to: usize) {
        let mut start = 0;
        while start <= to && start < text.len() {
            let end = match memchr::memchr(b'\n', &text[start..]) {
                Some(at) if start + at <= to => start + at,
                _ => to,
            };
            // Only a line that starts with a marker's character is looked at closer.
            if matches!(text[start], b'<' | b'=' | b'>') {
                match marker_kind(text, start, end) {
                    Some(Marker::Start) => self.open.push(false),
                    Some(Marker::Separator) => {
                        if let Some(open) = self.open.last_mut() {
                            *open = true;
                        }
                    }
                    Some(Marker::End) if self.open.pop() == Some(true) => self.count += 1,
                    Some(Marker::End) | None => {}
                }
            }
            start = end + 1;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Marker {
    Start,
    Separator,
    End,
}

/// Which marker the line from `start` to `end` (its newline) is, if it's one.
fn marker_kind(text: &[u8], start: usize, end: usize) -> Option<Marker> {
    // Without the line's CR, for a file with CRLF line ends.
    let last = if end > 0 && text.get(end - 1) == Some(&b'\r') {
        end - 1
    } else {
        end
    };
    let char = text[start];
    let mut run = start;
    while run < last && text[run] == char {
        run += 1;
    }
    if run - start < 7 {
        return None;
    }
    if char == b'=' {
        return (run == last).then_some(Marker::Separator);
    }
    // A label may follow, after a space: `<<<<<<< HEAD`.
    if run != last && !is_space(text[run]) {
        return None;
    }
    Some(if char == b'<' {
        Marker::Start
    } else {
        Marker::End
    })
}

/// Whether `byte`, as a Latin-1 character, is whitespace as JavaScript's `\s` has it.
fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r' | 0xa0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_the_conflicts_git_leaves_in_each_conflict_style() {
        let merge = "a\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> side\nb\n";
        let diff3 = "<<<<<<< HEAD\nours\n||||||| base\nbase\n=======\ntheirs\n>>>>>>> side\n";
        let zdiff3 = "<<<<<<< HEAD\nours\n||||||| base\n=======\n>>>>>>> side\n";
        assert_eq!(count_conflicts(merge), 1);
        assert_eq!(count_conflicts(diff3), 1);
        assert_eq!(count_conflicts(zdiff3), 1);
        assert_eq!(count_conflicts(&format!("{merge}{diff3}{zdiff3}")), 3);
    }

    #[test]
    fn reads_crlf_line_ends_and_a_file_that_ends_without_one() {
        assert_eq!(
            count_conflicts("<<<<<<< HEAD\r\na\r\n=======\r\nb\r\n>>>>>>> side\r\n"),
            1
        );
        assert_eq!(
            count_conflicts("<<<<<<< HEAD\na\n=======\nb\n>>>>>>> side"),
            1
        );
    }

    #[test]
    fn counts_conflicts_inside_conflicts_once_each() {
        let nested = [
            "<<<<<<< HEAD",
            "<<<<<<<<< Temporary merge branch 1",
            "a",
            "=========",
            "b",
            ">>>>>>>>> Temporary merge branch 2",
            "=======",
            "c",
            ">>>>>>> side",
            "",
        ]
        .join("\n");
        assert_eq!(count_conflicts(&nested), 2);
    }

    #[test]
    fn doesnt_take_what_only_looks_like_markers_for_a_conflict() {
        assert_eq!(count_conflicts(""), 0);
        assert_eq!(count_conflicts("Title\n=======\n\ntext\n"), 0);
        assert_eq!(count_conflicts("<<<<<<< HEAD\nunfinished\n"), 0);
        assert_eq!(count_conflicts("<<<<<< short\n=======\n>>>>>> short\n"), 0);
        assert_eq!(count_conflicts("<<<<<<<HEAD\n=======\n>>>>>>>side\n"), 0);
        assert_eq!(
            count_conflicts("<<<<<<< HEAD\nno separator\n>>>>>>> side\n"),
            0
        );
    }

    #[test]
    fn counts_the_same_however_the_text_comes_in_pieces() {
        let text = "a\n<<<<<<< HEAD\nours\n||||||| base\nbase\n=======\ntheirs\n>>>>>>> side\nb\n<<<<<<< x\n=======\n>>>>>>> y";
        for at in 0..=text.len() {
            let mut counter = ConflictCounter::new();
            counter.push(&text.as_bytes()[..at]);
            counter.push(&text.as_bytes()[at..]);
            assert_eq!(counter.end(), 2, "split at {at}");
        }
    }

    #[test]
    fn keeps_only_the_start_of_a_line_too_long_to_be_a_markers() {
        let mut counter = ConflictCounter::new();
        counter.push(b"<<<<<<< HEAD\nours\n");
        // A separator's run of = signs, longer than any marker line is kept: not a separator.
        counter.push("=".repeat(10_000).as_bytes());
        counter.push(b"\ntheirs\n>>>>>>> side\n");
        assert_eq!(counter.end(), 0);
    }

    #[test]
    fn looks_for_a_nul_in_the_first_8000_bytes_as_git_does() {
        assert!(!is_binary(b"text\n"));
        assert!(is_binary(&[1, 0, 2]));
        let mut late = vec![97u8; 8000];
        late.push(0);
        assert!(!is_binary(&late));
    }
}
