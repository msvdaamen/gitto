//! Builds the patch that stages or unstages some lines of a file, from git's own patch of its
//! changes: the one the user picked them from. Nothing is diffed again here; lines are only kept,
//! dropped, or kept as they are on both sides, and the hunk headers counted again to match.
//!
//! Ported from `lines.ts`.

use std::sync::LazyLock;

use regex::Regex;

use super::schema::{LineRange, LineSelection};
use crate::core::errors::{Error, Result, StageAction};

/// Which way the lines go: staged, from the unstaged changes (the index compared to the working
/// tree), or unstaged, from the staged ones (HEAD compared to the index). Either way, the patch
/// built is applied to the index: as it is to stage, in reverse to unstage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineAction {
    Stage,
    Unstage,
}

impl LineAction {
    /// What the lines are once moved, in messages: "staged" or "unstaged".
    pub fn verb(self) -> StageAction {
        match self {
            LineAction::Stage => StageAction::Staged,
            LineAction::Unstage => StageAction::Unstaged,
        }
    }
}

/// One line of a hunk, without its marker; `text` keeps a CR before the line's end, if it has one.
#[derive(Debug, Clone, Copy)]
struct PatchLine<'a> {
    kind: char,
    text: &'a str,
    /// It's the last of its side of the file, without a newline ("\ No newline at end of file").
    no_eol: bool,
}

#[derive(Debug)]
struct Hunk<'a> {
    old_start: i64,
    old_count: i64,
    new_start: i64,
    new_count: i64,
    /// What follows the second `@@`, e.g. the function the hunk is in.
    heading: &'a str,
    lines: Vec<PatchLine<'a>>,
}

/// A hunk's header. As JavaScript reads it: `\d` only ASCII digits, and `.` no line ends.
static HUNK_HEADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^@@ -([0-9]+)(?:,([0-9]+))? \+([0-9]+)(?:,([0-9]+))? @@([^\n\r\u{2028}\u{2029}]*)$",
    )
    .expect("a valid regex")
});

const NO_EOL: &str = "\\ No newline at end of file";

/// The patch that stages (or unstages, `action`) the lines `selection` picks from `patch`, git's
/// patch of one file's unstaged (or staged) changes; `None` if none of them are changes.
///
/// To stage, a picked line is kept as it is; an added line that isn't picked is dropped, and a
/// removed one stays, as an unchanged line: it's still in the index. To unstage, the other way
/// around: the patch is applied in reverse, from its new side (the index) to its old side, so an
/// added line that isn't picked stays, unchanged, and a removed one is dropped.
///
/// Only lines move. A change of the file's mode stays where it is, and so does a rename once it's
/// staged: lines are unstaged from the renamed file. An unstaged rename (of a file added with
/// `--intent-to-add`) is staged with the lines, which are only there on top of the old file's.
/// A file that's added or deleted stays so if all of it moves, and is changed otherwise.
///
/// Fails with `Error::LinesNotStageable` for a patch whose lines can't be picked: a binary file's,
/// a link's or a submodule's, one that changed type, or one without changed lines.
///
/// `patch` can be a file's bytes read as Latin-1, one character per byte (see `apply_lines`): the
/// patch built is too, as all it adds is ASCII.
pub fn lines_patch(
    patch: &str,
    selection: &LineSelection,
    action: LineAction,
) -> Result<Option<String>> {
    let verb = action.verb();
    let (header, hunks) = parse_patch(patch, verb)?;
    let picked = build_hunks(&hunks, selection, action);
    if picked.is_empty() {
        return Ok(None);
    }

    let file = parse_header(&header, verb)?;
    // Whether the file is still added, or deleted, as a whole: all of its lines move.
    let added = file.minus == "/dev/null" && picked.iter().all(|hunk| hunk.old_count == 0);
    let deleted = file.plus == "/dev/null" && picked.iter().all(|hunk| hunk.new_count == 0);
    let has_rename = !file.rename.is_empty();
    // Unstaging lines leaves the rename staged: they're unstaged from the renamed file.
    let renamed = has_rename && action == LineAction::Stage;
    let plus = if file.plus == "/dev/null" && !deleted {
        other_side(file.minus, "b/")?
    } else {
        file.plus.to_owned()
    };
    let minus = if (file.minus == "/dev/null" && !added) || (has_rename && !renamed) {
        other_side(&plus, "a/")?
    } else {
        file.minus.to_owned()
    };

    let mut lines: Vec<String> = vec![if has_rename && !renamed {
        format!("diff --git {} {}", without_tab(&minus), without_tab(&plus))
    } else {
        file.diff.to_owned()
    }];
    if added && let Some(created) = file.created {
        lines.push(created.to_owned());
    }
    if deleted && let Some(deleted) = file.deleted {
        lines.push(deleted.to_owned());
    }
    if renamed {
        lines.extend(file.rename.iter().map(|line| (*line).to_owned()));
    }
    lines.push(format!("--- {minus}"));
    lines.push(format!("+++ {plus}"));
    for hunk in &picked {
        lines.push(format!(
            "@@ -{} +{} @@{}",
            range(hunk.old_start, hunk.old_count),
            range(hunk.new_start, hunk.new_count),
            hunk.heading
        ));
        for line in &hunk.lines {
            lines.push(format!("{}{}", line.kind, line.text));
            if line.no_eol {
                lines.push(NO_EOL.to_owned());
            }
        }
    }
    Ok(Some(format!("{}\n", lines.join("\n"))))
}

/// A hunk header's range, as git writes it: without the count when it's 1.
fn range(start: i64, count: i64) -> String {
    if count == 1 {
        start.to_string()
    } else {
        format!("{start},{count}")
    }
}

/// The first `count` characters of `text`, for a message.
fn prefix(text: &str, count: usize) -> &str {
    match text.char_indices().nth(count) {
        Some((end, _)) => &text[..end],
        None => text,
    }
}

/// A hunk header's number; one too large to count is a patch git didn't write.
fn number(digits: &str) -> Result<i64> {
    digits
        .parse()
        .map_err(|_| Error::Bug(format!("Unexpected number in a hunk header: {digits}")))
}

/// Splits `patch` into its header's lines and its hunks.
fn parse_patch(patch: &str, verb: StageAction) -> Result<(Vec<&str>, Vec<Hunk<'_>>)> {
    let mut lines: Vec<&str> = patch.split('\n').collect();
    // The newline that ends the last line.
    if lines.last() == Some(&"") {
        lines.pop();
    }
    let mut i = 0;
    let mut header = Vec::new();
    while i < lines.len() && !lines[i].starts_with("@@ ") {
        header.push(lines[i]);
        i += 1;
    }
    if header
        .iter()
        .any(|line| line.starts_with("Binary files ") || *line == "GIT binary patch")
    {
        return Err(Error::LinesNotStageable(format!(
            "A binary file's changes can only be {verb} whole."
        )));
    }

    let mut hunks = Vec::new();
    while i < lines.len() {
        // A second file, after the first one's hunks: git has a file that changed type (from a
        // link to a file, say) as one deleted and another added.
        let Some(captures) = HUNK_HEADER.captures(lines[i]) else {
            return Err(Error::LinesNotStageable(format!(
                "A file that changed type can only be {verb} whole."
            )));
        };
        i += 1;
        let count = |group: usize| match captures.get(group) {
            Some(digits) => number(digits.as_str()),
            None => Ok(1),
        };
        let mut hunk = Hunk {
            old_start: number(&captures[1])?,
            old_count: count(2)?,
            new_start: number(&captures[3])?,
            new_count: count(4)?,
            heading: captures.get(5).map_or("", |heading| heading.as_str()),
            lines: Vec::new(),
        };
        // Read by the header's counts: with `diff.suppressBlankEmpty`, an empty unchanged line is
        // left empty, without its marker.
        let mut old_left = hunk.old_count;
        let mut new_left = hunk.new_count;
        while i < lines.len() && (old_left > 0 || new_left > 0 || lines[i].starts_with('\\')) {
            let line = lines[i];
            i += 1;
            let mut chars = line.chars();
            let marker = chars.next().unwrap_or(' ');
            if marker == '\\' {
                if let Some(last) = hunk.lines.last_mut() {
                    last.no_eol = true;
                }
                continue;
            }
            if marker != ' ' && marker != '-' && marker != '+' {
                return Err(Error::Bug(format!(
                    "Unexpected line in a hunk: {}",
                    prefix(line, 40)
                )));
            }
            if marker != '+' {
                old_left -= 1;
            }
            if marker != '-' {
                new_left -= 1;
            }
            hunk.lines.push(PatchLine {
                kind: marker,
                text: chars.as_str(),
                no_eol: false,
            });
        }
        hunks.push(hunk);
    }
    if hunks.is_empty() {
        return Err(Error::LinesNotStageable(format!(
            "This file has no changed lines to be {verb}."
        )));
    }
    Ok((header, hunks))
}

/// What the lines are built from, of a patch's header.
struct FileHeader<'a> {
    /// Its `diff --git` line.
    diff: &'a str,
    /// The paths on its `---` and `+++` lines, as git wrote them, or `/dev/null`.
    minus: &'a str,
    plus: &'a str,
    /// Its `new file mode` or `deleted file mode` line, for a file added or deleted.
    created: Option<&'a str>,
    deleted: Option<&'a str>,
    /// Its `rename from` and `rename to` lines, for a renamed file.
    rename: Vec<&'a str>,
}

/// Reads the parts of a patch's header that are kept. The `index` line isn't: its object names are
/// of the whole file, not of the lines picked (git doesn't check them in a patch of text), and the
/// mode on it, like the `old mode` and `new mode` of a mode change, would move the mode too.
fn parse_header<'a>(header: &[&'a str], verb: StageAction) -> Result<FileHeader<'a>> {
    let mut file = FileHeader {
        diff: header.first().copied().unwrap_or(""),
        minus: "",
        plus: "",
        created: None,
        deleted: None,
        rename: Vec::new(),
    };
    for &line in header {
        // A link's contents are the path it points to, and a submodule's the commit it's at.
        let link = line.ends_with(" 120000");
        if link || line.ends_with(" 160000") {
            let what = if link { "A link" } else { "A submodule" };
            return Err(Error::LinesNotStageable(format!(
                "{what} can only be {verb} whole."
            )));
        }
        if let Some(path) = line.strip_prefix("--- ") {
            file.minus = path;
        } else if let Some(path) = line.strip_prefix("+++ ") {
            file.plus = path;
        } else if line.starts_with("new file mode ") {
            file.created = Some(line);
        } else if line.starts_with("deleted file mode ") {
            file.deleted = Some(line);
        } else if line.starts_with("rename from ") || line.starts_with("rename to ") {
            file.rename.push(line);
        }
    }
    if !file.diff.starts_with("diff --git ") || file.minus.is_empty() || file.plus.is_empty() {
        return Err(Error::Bug("The patch has no file header.".to_owned()));
    }
    Ok(file)
}

/// `path` from a `---` or `+++` line (`a/x` or `b/x`, or quoted, `"a/x\ty"`), as the other side has
/// it: with `prefix`.
fn other_side(path: &str, prefix: &str) -> Result<String> {
    let quoted = path.starts_with('"');
    let name = if quoted { &path[1..] } else { path };
    let Some(rest) = name.strip_prefix("a/").or_else(|| name.strip_prefix("b/")) else {
        return Err(Error::Bug(format!("Unexpected path in a patch: {path}")));
    };
    Ok(format!("{}{prefix}{rest}", if quoted { "\"" } else { "" }))
}

/// A path from a `---` or `+++` line without the tab git ends one with that has a space, as on the
/// `diff --git` line.
fn without_tab(path: &str) -> &str {
    path.strip_suffix('\t').unwrap_or(path)
}

/// The hunks with the lines picked, renumbered, without the ones left with no changes.
fn build_hunks<'a>(
    hunks: &[Hunk<'a>],
    selection: &LineSelection,
    action: LineAction,
) -> Vec<Hunk<'a>> {
    let mut deletions = Picked::new(&selection.deletions);
    let mut additions = Picked::new(&selection.additions);
    let mut built = Vec::new();
    // How many more lines the new side of the hunks built so far has than the old one.
    let mut grown: i64 = 0;
    for hunk in hunks {
        let mut lines = Vec::new();
        let mut old_line = hunk.old_start;
        let mut new_line = hunk.new_start;
        for &line in &hunk.lines {
            if line.kind == ' ' {
                old_line += 1;
                new_line += 1;
                lines.push(line);
            } else if line.kind == '-' {
                let picked = deletions.has(old_line);
                old_line += 1;
                if picked {
                    lines.push(line);
                } else if action == LineAction::Stage {
                    lines.push(PatchLine { kind: ' ', ..line });
                }
            } else {
                let picked = additions.has(new_line);
                new_line += 1;
                if picked {
                    lines.push(line);
                } else if action == LineAction::Unstage {
                    lines.push(PatchLine { kind: ' ', ..line });
                }
            }
        }
        if lines.iter().all(|line| line.kind == ' ') {
            continue;
        }

        let ended = end_of_file(&lines);
        let old_count = ended.iter().filter(|line| line.kind != '+').count() as i64;
        let new_count = ended.iter().filter(|line| line.kind != '-').count() as i64;
        // The side the patch is applied to keeps its lines, and their numbers; the other side's are
        // moved by the lines the hunks before added or removed. A side without lines in the hunk is
        // numbered by the line before it.
        let (old_start, new_start) = if action == LineAction::Stage {
            let before = hunk.old_start - i64::from(hunk.old_count > 0) + grown;
            (hunk.old_start, before + i64::from(new_count > 0))
        } else {
            let before = hunk.new_start - i64::from(hunk.new_count > 0) - grown;
            (before + i64::from(old_count > 0), hunk.new_start)
        };
        grown += new_count - old_count;
        built.push(Hunk {
            old_start,
            old_count,
            new_start,
            new_count,
            heading: hunk.heading,
            lines: ended,
        });
    }
    built
}

/// `lines` with a newline at the end of any line that's no longer the last of its side: one without
/// ends the file. That's a removed line kept as an unchanged one (to stage) with added ones picked
/// after it, say: it's still the last of the old side, but the added lines follow it on the new
/// side, so it's removed without a newline and added with one. The side the patch is applied to
/// never changes this way, as all of its lines are kept.
fn end_of_file<'a>(lines: &[PatchLine<'a>]) -> Vec<PatchLine<'a>> {
    let mut ended = Vec::with_capacity(lines.len());
    for (i, &line) in lines.iter().enumerate() {
        if !line.no_eol {
            ended.push(line);
            continue;
        }
        let after = &lines[i + 1..];
        let old_after = after.iter().any(|next| next.kind != '+');
        let new_after = after.iter().any(|next| next.kind != '-');
        if line.kind == '-' {
            ended.push(PatchLine {
                no_eol: !old_after,
                ..line
            });
        } else if line.kind == '+' {
            ended.push(PatchLine {
                no_eol: !new_after,
                ..line
            });
        } else if old_after == new_after {
            ended.push(PatchLine {
                no_eol: !old_after,
                ..line
            });
        } else {
            ended.push(PatchLine {
                kind: '-',
                text: line.text,
                no_eol: !old_after,
            });
            ended.push(PatchLine {
                kind: '+',
                text: line.text,
                no_eol: !new_after,
            });
        }
    }
    ended
}

/// Whether a line is in some ranges, for line numbers asked in increasing order: in one pass over
/// the ranges, however many lines and ranges there are.
struct Picked {
    /// `(start, end)`, by their start.
    ranges: Vec<(i64, i64)>,
    next: usize,
}

impl Picked {
    fn new(ranges: &[LineRange]) -> Self {
        let line = |number: u64| i64::try_from(number).unwrap_or(i64::MAX);
        let mut ranges: Vec<(i64, i64)> = ranges
            .iter()
            .map(|range| (line(range.start()), line(range.end())))
            .collect();
        ranges.sort_by_key(|&(start, _)| start);
        Picked { ranges, next: 0 }
    }

    fn has(&mut self, line: i64) -> bool {
        while self.next < self.ranges.len() && self.ranges[self.next].1 < line {
            self.next += 1;
        }
        // Ranges can overlap: a later one can end after a longer one that starts before it.
        self.ranges[self.next..]
            .iter()
            .take_while(|&&(start, _)| start <= line)
            .any(|&(_, end)| end >= line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A patch from lines, each ending in a newline.
    fn patch(lines: &[&str]) -> String {
        format!("{}\n", lines.join("\n"))
    }

    /// `HEADER` (or another start) followed by `lines`.
    fn with(start: &[&str], lines: &[&str]) -> String {
        patch(&[start, lines].concat())
    }

    /// Picks single lines: removed ones by their old number, added ones by their new one.
    fn pick(deletions: &[u64], additions: &[u64]) -> LineSelection {
        let single = |line: &u64| LineRange::new(*line, *line).unwrap();
        LineSelection {
            deletions: deletions.iter().map(single).collect(),
            additions: additions.iter().map(single).collect(),
        }
    }

    fn stage(patch: &str, selection: LineSelection) -> Option<String> {
        lines_patch(patch, &selection, LineAction::Stage).unwrap()
    }

    fn unstage(patch: &str, selection: LineSelection) -> Option<String> {
        lines_patch(patch, &selection, LineAction::Unstage).unwrap()
    }

    const HEADER: [&str; 4] = [
        "diff --git a/f.txt b/f.txt",
        "index 1111111..2222222 100644",
        "--- a/f.txt",
        "+++ b/f.txt",
    ];
    const BUILT: [&str; 3] = ["diff --git a/f.txt b/f.txt", "--- a/f.txt", "+++ b/f.txt"];

    /// a b c d e → a B C d e x
    fn changed() -> String {
        with(
            &HEADER,
            &[
                "@@ -1,5 +1,6 @@ heading",
                " a",
                "-b",
                "-c",
                "+B",
                "+C",
                " d",
                " e",
                "+x",
            ],
        )
    }

    // Staging lines.

    #[test]
    fn keeps_the_picked_lines_drops_added_ones_and_keeps_removed_ones_unchanged() {
        assert_eq!(
            stage(&changed(), pick(&[2], &[3])),
            Some(with(
                &BUILT,
                &[
                    "@@ -1,5 +1,5 @@ heading",
                    " a",
                    "-b",
                    " c",
                    "+C",
                    " d",
                    " e"
                ]
            ))
        );
    }

    #[test]
    fn stages_a_whole_hunk_as_it_is_but_for_its_index_line() {
        assert_eq!(
            stage(&changed(), pick(&[2, 3], &[2, 3, 6])),
            Some(with(
                &BUILT,
                &[
                    "@@ -1,5 +1,6 @@ heading",
                    " a",
                    "-b",
                    "-c",
                    "+B",
                    "+C",
                    " d",
                    " e",
                    "+x"
                ]
            ))
        );
    }

    #[test]
    fn stages_only_removed_lines_or_only_added_ones() {
        assert_eq!(
            stage(&changed(), pick(&[3], &[])),
            Some(with(
                &BUILT,
                &["@@ -1,5 +1,4 @@ heading", " a", " b", "-c", " d", " e"]
            ))
        );
        assert_eq!(
            stage(&changed(), pick(&[], &[6])),
            Some(with(
                &BUILT,
                &[
                    "@@ -1,5 +1,6 @@ heading",
                    " a",
                    " b",
                    " c",
                    " d",
                    " e",
                    "+x"
                ]
            ))
        );
    }

    #[test]
    fn takes_ranges_which_can_overlap_and_come_in_any_order() {
        let selection = LineSelection {
            deletions: vec![LineRange::new(3, 3).unwrap()],
            additions: vec![
                LineRange::new(6, 6).unwrap(),
                LineRange::new(2, 3).unwrap(),
                LineRange::new(3, 3).unwrap(),
            ],
        };
        assert_eq!(
            stage(&changed(), selection),
            Some(with(
                &BUILT,
                &[
                    "@@ -1,5 +1,7 @@ heading",
                    " a",
                    " b",
                    "-c",
                    "+B",
                    "+C",
                    " d",
                    " e",
                    "+x"
                ]
            ))
        );
    }

    #[test]
    fn has_nothing_to_stage_without_changed_lines_picked() {
        assert_eq!(stage(&changed(), pick(&[], &[])), None);
        // Unchanged lines, and numbers past the hunk's.
        assert_eq!(stage(&changed(), pick(&[1, 4, 9], &[1, 4, 9])), None);
    }

    #[test]
    fn numbers_the_new_side_of_a_hunk_after_the_lines_staged_before_it() {
        // Lines 1–20, with 3 changed and 2 added after 15.
        let two_hunks = with(
            &HEADER,
            &[
                "@@ -1,6 +1,6 @@",
                " 1",
                " 2",
                "-3",
                "+three",
                " 4",
                " 5",
                " 6",
                "@@ -13,6 +13,8 @@",
                " 13",
                " 14",
                " 15",
                "+15a",
                "+15b",
                " 16",
                " 17",
                " 18",
            ],
        );
        // The first hunk left out: the second keeps its numbers.
        assert_eq!(
            stage(&two_hunks, pick(&[], &[17])),
            Some(with(
                &BUILT,
                &[
                    "@@ -13,6 +13,7 @@",
                    " 13",
                    " 14",
                    " 15",
                    "+15b",
                    " 16",
                    " 17",
                    " 18"
                ]
            ))
        );
        // A line removed in the first: the second's new side starts a line earlier.
        assert_eq!(
            stage(&two_hunks, pick(&[3], &[16])),
            Some(with(
                &BUILT,
                &[
                    "@@ -1,6 +1,5 @@",
                    " 1",
                    " 2",
                    "-3",
                    " 4",
                    " 5",
                    " 6",
                    "@@ -13,6 +12,7 @@",
                    " 13",
                    " 14",
                    " 15",
                    "+15a",
                    " 16",
                    " 17",
                    " 18",
                ]
            ))
        );
    }

    #[test]
    fn numbers_a_hunk_without_lines_on_one_side_by_the_line_before_it() {
        // A line added at the top of an empty file, and one removed from the end of another.
        let added = with(&HEADER, &["@@ -0,0 +1,2 @@", "+a", "+b"]);
        assert_eq!(
            stage(&added, pick(&[], &[2])),
            Some(with(&BUILT, &["@@ -0,0 +1 @@", "+b"]))
        );
        let removed = with(&HEADER, &["@@ -3,2 +2,0 @@", "-c", "-d"]);
        assert_eq!(
            stage(&removed, pick(&[4], &[])),
            Some(with(&BUILT, &["@@ -3,2 +3 @@", " c", "-d"]))
        );
    }

    #[test]
    fn keeps_a_cr_at_the_end_of_a_line() {
        let crlf = with(&HEADER, &["@@ -1,2 +1,2 @@", " a\r", "-b\r", "+B\r"]);
        assert_eq!(
            stage(&crlf, pick(&[2], &[2])),
            Some(with(&BUILT, &["@@ -1,2 +1,2 @@", " a\r", "-b\r", "+B\r"]))
        );
    }

    #[test]
    fn reads_an_empty_unchanged_line_without_its_marker_as_diff_suppress_blank_empty_has_it() {
        let blank = with(&HEADER, &["@@ -1,3 +1,3 @@", " a", "", "-c", "+C"]);
        assert_eq!(
            stage(&blank, pick(&[3], &[3])),
            Some(with(&BUILT, &["@@ -1,3 +1,3 @@", " a", " ", "-c", "+C"]))
        );
    }

    // At the end of a file without a newline.

    /// a b → a b x, b without a newline before.
    fn appended() -> String {
        with(
            &HEADER,
            &[
                "@@ -1,2 +1,3 @@",
                " a",
                "-b",
                "\\ No newline at end of file",
                "+b",
                "+x",
            ],
        )
    }

    #[test]
    fn removes_the_old_last_line_and_adds_it_back_with_a_newline_to_add_lines_after_it() {
        assert_eq!(
            stage(&appended(), pick(&[], &[3])),
            Some(with(
                &BUILT,
                &[
                    "@@ -1,2 +1,3 @@",
                    " a",
                    "-b",
                    "\\ No newline at end of file",
                    "+b",
                    "+x"
                ]
            ))
        );
    }

    #[test]
    fn keeps_it_unchanged_without_a_newline_when_nothings_added_after_it() {
        // b → B, both without a newline: only the removal staged.
        let replaced = with(
            &HEADER,
            &[
                "@@ -1,2 +1,2 @@",
                " a",
                "-b",
                "\\ No newline at end of file",
                "+B",
                "\\ No newline at end of file",
            ],
        );
        assert_eq!(
            stage(&replaced, pick(&[2], &[])),
            Some(with(
                &BUILT,
                &["@@ -1,2 +1 @@", " a", "-b", "\\ No newline at end of file"]
            ))
        );
        // Only the addition: b stays, and B is added after it.
        assert_eq!(
            stage(&replaced, pick(&[], &[2])),
            Some(with(
                &BUILT,
                &[
                    "@@ -1,2 +1,3 @@",
                    " a",
                    "-b",
                    "\\ No newline at end of file",
                    "+b",
                    "+B",
                    "\\ No newline at end of file",
                ]
            ))
        );
    }

    #[test]
    fn keeps_the_marker_of_an_added_last_line() {
        let no_eol = with(
            &HEADER,
            &["@@ -1 +1,2 @@", " a", "+b", "\\ No newline at end of file"],
        );
        assert_eq!(
            stage(&no_eol, pick(&[], &[2])),
            Some(with(
                &BUILT,
                &["@@ -1 +1,2 @@", " a", "+b", "\\ No newline at end of file"]
            ))
        );
    }

    #[test]
    fn unstages_a_removed_last_line_with_a_newline_when_unstaged_lines_follow_it() {
        // Staged: a b → a b x, b without a newline in HEAD. Unstaging the removal of b, but not
        // the lines added, puts b back before them: with a newline.
        assert_eq!(
            unstage(&appended(), pick(&[2], &[])),
            Some(with(&BUILT, &["@@ -1,4 +1,3 @@", " a", "-b", " b", " x"]))
        );
        // Unstaging x alone leaves b with its newline.
        assert_eq!(
            unstage(&appended(), pick(&[], &[3])),
            Some(with(&BUILT, &["@@ -1,2 +1,3 @@", " a", " b", "+x"]))
        );
    }

    // Unstaging lines.

    #[test]
    fn keeps_the_picked_lines_drops_removed_ones_and_keeps_added_ones_unchanged() {
        assert_eq!(
            unstage(&changed(), pick(&[2], &[3])),
            Some(with(
                &BUILT,
                &[
                    "@@ -1,6 +1,6 @@ heading",
                    " a",
                    "-b",
                    " B",
                    "+C",
                    " d",
                    " e",
                    " x"
                ]
            ))
        );
    }

    #[test]
    fn numbers_the_old_side_of_a_hunk_after_the_lines_unstaged_before_it() {
        let two_hunks = with(
            &HEADER,
            &[
                "@@ -1,3 +1,4 @@",
                " 1",
                "+1a",
                " 2",
                " 3",
                "@@ -10,3 +11,3 @@",
                " 10",
                "-11",
                "+eleven",
                " 12",
            ],
        );
        // The first hunk's line stays staged: the second's old side is the index's, less it.
        assert_eq!(
            unstage(&two_hunks, pick(&[11], &[12])),
            Some(with(
                &BUILT,
                &["@@ -11,3 +11,3 @@", " 10", "-11", "+eleven", " 12"]
            ))
        );
        // Unstaged too: the old side's lines are numbered as without it.
        assert_eq!(
            unstage(&two_hunks, pick(&[11], &[2, 12])),
            Some(with(
                &BUILT,
                &[
                    "@@ -1,3 +1,4 @@",
                    " 1",
                    "+1a",
                    " 2",
                    " 3",
                    "@@ -10,3 +11,3 @@",
                    " 10",
                    "-11",
                    "+eleven",
                    " 12",
                ]
            ))
        );
    }

    // Whole files.

    fn added_file() -> String {
        patch(&[
            "diff --git a/new.txt b/new.txt",
            "new file mode 100755",
            "index 0000000..2222222",
            "--- /dev/null",
            "+++ b/new.txt",
            "@@ -0,0 +1,3 @@",
            "+a",
            "+b",
            "+c",
        ])
    }

    fn deleted_file() -> String {
        patch(&[
            "diff --git a/old.txt b/old.txt",
            "deleted file mode 100644",
            "index 1111111..0000000",
            "--- a/old.txt",
            "+++ /dev/null",
            "@@ -1,3 +0,0 @@",
            "-a",
            "-b",
            "-c",
        ])
    }

    #[test]
    fn stages_some_lines_of_a_new_file_as_a_new_file_with_its_mode() {
        assert_eq!(
            stage(&added_file(), pick(&[], &[1, 3])),
            Some(patch(&[
                "diff --git a/new.txt b/new.txt",
                "new file mode 100755",
                "--- /dev/null",
                "+++ b/new.txt",
                "@@ -0,0 +1,2 @@",
                "+a",
                "+c",
            ]))
        );
    }

    #[test]
    fn unstages_a_new_files_lines_from_it_or_the_file_if_theyre_all_of_it() {
        assert_eq!(
            unstage(&added_file(), pick(&[], &[2])),
            Some(patch(&[
                "diff --git a/new.txt b/new.txt",
                "--- a/new.txt",
                "+++ b/new.txt",
                "@@ -1,2 +1,3 @@",
                " a",
                "+b",
                " c",
            ]))
        );
        assert_eq!(
            unstage(&added_file(), pick(&[], &[1, 2, 3])),
            Some(patch(&[
                "diff --git a/new.txt b/new.txt",
                "new file mode 100755",
                "--- /dev/null",
                "+++ b/new.txt",
                "@@ -0,0 +1,3 @@",
                "+a",
                "+b",
                "+c",
            ]))
        );
    }

    #[test]
    fn stages_a_deleted_files_lines_from_it_or_its_deletion_if_theyre_all_of_it() {
        assert_eq!(
            stage(&deleted_file(), pick(&[2], &[])),
            Some(patch(&[
                "diff --git a/old.txt b/old.txt",
                "--- a/old.txt",
                "+++ b/old.txt",
                "@@ -1,3 +1,2 @@",
                " a",
                "-b",
                " c",
            ]))
        );
        assert_eq!(
            stage(&deleted_file(), pick(&[1, 2, 3], &[])),
            Some(patch(&[
                "diff --git a/old.txt b/old.txt",
                "deleted file mode 100644",
                "--- a/old.txt",
                "+++ /dev/null",
                "@@ -1,3 +0,0 @@",
                "-a",
                "-b",
                "-c",
            ]))
        );
    }

    #[test]
    fn unstages_some_lines_of_a_deleted_file_as_a_deleted_file_of_only_those() {
        assert_eq!(
            unstage(&deleted_file(), pick(&[1, 3], &[])),
            Some(patch(&[
                "diff --git a/old.txt b/old.txt",
                "deleted file mode 100644",
                "--- a/old.txt",
                "+++ /dev/null",
                "@@ -1,2 +0,0 @@",
                "-a",
                "-c",
            ]))
        );
    }

    fn renamed_file() -> String {
        patch(&[
            "diff --git a/old name.txt b/new name.txt",
            "similarity index 80%",
            "rename from old name.txt",
            "rename to new name.txt",
            "index 1111111..2222222 100644",
            "--- a/old name.txt\t",
            "+++ b/new name.txt\t",
            "@@ -1,2 +1,2 @@",
            " a",
            "-b",
            "+B",
        ])
    }

    #[test]
    fn stages_lines_of_a_rename_with_the_rename() {
        assert_eq!(
            stage(&renamed_file(), pick(&[], &[2])),
            Some(patch(&[
                "diff --git a/old name.txt b/new name.txt",
                "rename from old name.txt",
                "rename to new name.txt",
                "--- a/old name.txt\t",
                "+++ b/new name.txt\t",
                "@@ -1,2 +1,3 @@",
                " a",
                " b",
                "+B",
            ]))
        );
    }

    #[test]
    fn unstages_lines_of_a_rename_from_the_renamed_file_which_stays_renamed() {
        assert_eq!(
            unstage(&renamed_file(), pick(&[2], &[2])),
            Some(patch(&[
                "diff --git a/new name.txt b/new name.txt",
                "--- a/new name.txt\t",
                "+++ b/new name.txt\t",
                "@@ -1,2 +1,2 @@",
                " a",
                "-b",
                "+B",
            ]))
        );
    }

    #[test]
    fn names_a_file_as_git_quoted_it() {
        let quoted = patch(&[
            r#"diff --git "a/tab\there.txt" "b/tab\there.txt""#,
            "new file mode 100644",
            "--- /dev/null",
            r#"+++ "b/tab\there.txt""#,
            "@@ -0,0 +1,2 @@",
            "+a",
            "+b",
        ]);
        assert_eq!(
            unstage(&quoted, pick(&[], &[1])),
            Some(patch(&[
                r#"diff --git "a/tab\there.txt" "b/tab\there.txt""#,
                r#"--- "a/tab\there.txt""#,
                r#"+++ "b/tab\there.txt""#,
                "@@ -1 +1,2 @@",
                "+a",
                " b",
            ]))
        );
    }

    #[test]
    fn leaves_a_change_of_mode_out() {
        let chmod = patch(&[
            "diff --git a/f.txt b/f.txt",
            "old mode 100644",
            "new mode 100755",
            "index 1111111..2222222",
            "--- a/f.txt",
            "+++ b/f.txt",
            "@@ -1 +1 @@",
            "-a",
            "+A",
        ]);
        for action in [LineAction::Stage, LineAction::Unstage] {
            assert_eq!(
                lines_patch(&chmod, &pick(&[1], &[1]), action).unwrap(),
                Some(with(&BUILT, &["@@ -1 +1 @@", "-a", "+A"]))
            );
        }
    }

    // What can't be staged by line.

    /// Expects `input` to be refused with `message`, both to stage and unstage, as VERB says.
    fn rejects(input: &str, message: &str) {
        for (action, verb) in [
            (LineAction::Stage, "staged"),
            (LineAction::Unstage, "unstaged"),
        ] {
            match lines_patch(input, &pick(&[1], &[1]), action) {
                Err(Error::LinesNotStageable(got)) => {
                    assert_eq!(got, message.replace("VERB", verb));
                }
                other => panic!("expected LinesNotStageable, got {other:?}"),
            }
        }
    }

    #[test]
    fn rejects_a_binary_file() {
        rejects(
            &with(&HEADER[..2], &["Binary files a/f.txt and b/f.txt differ"]),
            "A binary file's changes can only be VERB whole.",
        );
        rejects(
            &with(
                &HEADER[..2],
                &["GIT binary patch", "literal 3", "Hc$@<O00001"],
            ),
            "A binary file's changes can only be VERB whole.",
        );
    }

    #[test]
    fn rejects_a_link_and_a_submodule() {
        rejects(
            &patch(&[
                "diff --git a/l b/l",
                "index 1111111..2222222 120000",
                "--- a/l",
                "+++ b/l",
                "@@ -1 +1 @@",
                "-x",
                "\\ No newline at end of file",
                "+y",
                "\\ No newline at end of file",
            ]),
            "A link can only be VERB whole.",
        );
        rejects(
            &patch(&[
                "diff --git a/m b/m",
                "index 1111111..2222222 160000",
                "--- a/m",
                "+++ b/m",
                "@@ -1 +1 @@",
                "-Subproject commit 1111111111111111111111111111111111111111",
                "+Subproject commit 2222222222222222222222222222222222222222",
            ]),
            "A submodule can only be VERB whole.",
        );
    }

    #[test]
    fn rejects_a_file_that_changed_type() {
        rejects(
            &patch(&[
                "diff --git a/f b/f",
                "deleted file mode 100644",
                "--- a/f",
                "+++ /dev/null",
                "@@ -1 +0,0 @@",
                "-a",
                "diff --git a/f b/f",
                "new file mode 120000",
                "--- /dev/null",
                "+++ b/f",
                "@@ -0,0 +1 @@",
                "+target",
                "\\ No newline at end of file",
            ]),
            "A file that changed type can only be VERB whole.",
        );
    }

    #[test]
    fn rejects_a_patch_without_changed_lines() {
        rejects(
            &patch(&["diff --git a/f b/f", "old mode 100644", "new mode 100755"]),
            "This file has no changed lines to be VERB.",
        );
        rejects("", "This file has no changed lines to be VERB.");
    }
}
