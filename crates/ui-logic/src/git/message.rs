//! Ported from `git/message.ts`: commit messages, as the commit form edits them.

/// A commit message as the commit form edits it: a one-line summary, and the rest.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommitMessage {
    pub summary: String,
    pub description: String,
}

impl CommitMessage {
    pub fn new(summary: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
            description: description.into(),
        }
    }
}

// Whitespace as git sees it: ASCII only, so e.g. a line of a no-break space isn't blank.

/// Whitespace within a line: `[ \t\v\f\r]`.
fn is_line_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | 0x0b | 0x0c | b'\r')
}

/// Any whitespace, line breaks too: `[ \t\n\v\f\r]`.
fn is_space(byte: u8) -> bool {
    byte == b'\n' || is_line_space(byte)
}

/// The end of the blank lines (`(?:[ \t\v\f\r]*\n)+`) from `start`; `start` if there are none.
fn blank_lines_end(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    let mut end = start;
    let mut at = start;
    loop {
        while at < bytes.len() && is_line_space(bytes[at]) {
            at += 1;
        }
        if at < bytes.len() && bytes[at] == b'\n' {
            at += 1;
            end = at;
        } else {
            return end;
        }
    }
}

/// `text` without the blank lines it starts with (`^(?:[ \t\v\f\r]*\n)+`).
fn skip_leading_blank_lines(text: &str) -> &str {
    &text[blank_lines_end(text, 0)..]
}

/// Where the first blank lines after a line are (`\n(?:[ \t\v\f\r]*\n)+`): from the line break
/// that ends that line to the end of the last blank line.
fn find_blank_lines(text: &str) -> Option<(usize, usize)> {
    text.bytes()
        .enumerate()
        .filter(|&(_, byte)| byte == b'\n')
        .find_map(|(index, _)| {
            let end = blank_lines_end(text, index + 1);
            (end > index + 1).then_some((index, end))
        })
}

/// `text` with its line breaks, and the whitespace around each, as one space each
/// (`[ \t\v\f\r]*\n[ \t\v\f\r]*` → ` `).
fn join_lines(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let last = lines.len() - 1;
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let mut line = *line;
            if index > 0 {
                line = line.trim_start_matches(|c: char| c.is_ascii() && is_line_space(c as u8));
            }
            if index < last {
                line = line.trim_end_matches(|c: char| c.is_ascii() && is_line_space(c as u8));
            }
            line
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `text` without whitespace at either end (`[ \t\n\v\f\r]`).
fn trim(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_ascii() && is_space(c as u8))
}

/// Splits a commit message into its subject, the first paragraph on one line as git shows it, and
/// the body after it.
pub fn split_message(message: &str) -> CommitMessage {
    let Parts { subject, body, .. } = parts(message);
    CommitMessage {
        summary: trim(&join_lines(subject)).to_owned(),
        description: trim_body(body).to_owned(),
    }
}

/// The message to commit: the summary, then a blank line and the description if there is one.
pub fn join_message(message: &CommitMessage) -> String {
    let body = trim_body(&message.description);
    let summary = trim(&message.summary);
    if body.is_empty() {
        summary.to_owned()
    } else {
        format!("{summary}\n\n{body}")
    }
}

/// `original` with the edits made to it as [`split_message`] split it, to be committed as it is. A
/// part left as it was, or only changed around its ends, is kept exactly as written, e.g. a subject
/// over several lines when only the description changed. Ends in a newline, as git's messages do.
pub fn edit_message(original: &str, edited: &CommitMessage) -> String {
    let Parts {
        subject,
        separator,
        body,
    } = parts(original);
    let split = split_message(original);
    let new_summary = trim(&edited.summary);
    let new_description = trim_body(&edited.description);
    if new_summary == split.summary && new_description == split.description {
        return original.to_owned();
    }
    let summary = if new_summary == split.summary {
        subject
    } else {
        new_summary
    };
    let description = if new_description == split.description {
        body
    } else {
        new_description
    };
    let mut message = if trim(description).is_empty() {
        summary.to_owned()
    } else {
        format!("{summary}{separator}{description}")
    };
    if !message.ends_with('\n') {
        message.push('\n');
    }
    message
}

/// A message's first paragraph, the blank lines after it, and the rest, as written.
struct Parts<'a> {
    subject: &'a str,
    separator: &'a str,
    body: &'a str,
}

/// The first paragraph, the blank lines after it, and the rest, as written. Blank lines before the
/// first paragraph are skipped, as git does.
fn parts(written: &str) -> Parts<'_> {
    let message = skip_leading_blank_lines(written);
    match find_blank_lines(message) {
        None => Parts {
            subject: message.trim_end_matches('\n'),
            separator: "\n\n",
            body: "",
        },
        Some((start, end)) => Parts {
            subject: &message[..start],
            separator: &message[start..end],
            body: &message[end..],
        },
    }
}

/// Drops blank lines around the body, but keeps its first line's indentation, e.g. for code.
fn trim_body(body: &str) -> &str {
    skip_leading_blank_lines(body).trim_end_matches(|c: char| c.is_ascii() && is_space(c as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    mod commit_messages {
        use super::*;

        #[test]
        fn splits_off_the_first_line_as_the_summary() {
            assert_eq!(
                split_message("Subject\n\nBody\n#123 kept"),
                CommitMessage::new("Subject", "Body\n#123 kept")
            );
            assert_eq!(split_message("Subject"), CommitMessage::new("Subject", ""));
        }

        #[test]
        fn puts_a_subject_over_several_lines_on_one_as_git_shows_it() {
            assert_eq!(
                split_message("Fix parser\nfor nested lists\n\nBody"),
                CommitMessage::new("Fix parser for nested lists", "Body")
            );
        }

        #[test]
        fn only_counts_a_line_of_ascii_whitespace_as_blank_as_git_does() {
            assert_eq!(
                split_message("Subject\n\u{a0}\nBody"),
                CommitMessage::new("Subject \u{a0} Body", "")
            );
        }

        #[test]
        fn skips_blank_lines_before_the_subject_as_git_does() {
            assert_eq!(
                split_message("\n \nSubject\n\nBody"),
                CommitMessage::new("Subject", "Body")
            );
        }

        #[test]
        fn joins_the_summary_and_the_description_with_a_blank_line() {
            assert_eq!(
                join_message(&CommitMessage::new(" Subject ", "\nBody\n")),
                "Subject\n\nBody"
            );
            assert_eq!(
                join_message(&CommitMessage::new("Subject", "  ")),
                "Subject"
            );
        }

        #[test]
        fn round_trips_a_message_indented_body_and_all() {
            let message = "Subject\n\n    code();\n\n## Notes";
            assert_eq!(join_message(&split_message(message)), message);
        }

        #[test]
        fn keeps_what_wasnt_edited_exactly_as_written() {
            let original = "Fix parser\nfor nested lists  \n\n\nBody  \n";
            let split = split_message(original);
            assert_eq!(edit_message(original, &split), original);
            assert_eq!(
                edit_message(
                    original,
                    &CommitMessage {
                        description: "New body".into(),
                        ..split.clone()
                    }
                ),
                "Fix parser\nfor nested lists  \n\n\nNew body\n"
            );
            assert_eq!(
                edit_message(
                    original,
                    &CommitMessage {
                        summary: "Fix the parser".into(),
                        ..split.clone()
                    }
                ),
                "Fix the parser\n\n\nBody  \n"
            );
            assert_eq!(
                edit_message(
                    original,
                    &CommitMessage {
                        summary: format!("{} ", split.summary),
                        ..split.clone()
                    }
                ),
                original
            );
            assert_eq!(
                edit_message("Subject\n", &split_message("Subject\n")),
                "Subject\n"
            );
            assert_eq!(
                edit_message("Subject", &CommitMessage::new("Subject", "Added")),
                "Subject\n\nAdded\n"
            );
        }
    }
}
