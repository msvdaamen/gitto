//! Builds the patch that stages or unstages some lines of a file, from git's own patch of its
//! changes: the one the user picked them from. Nothing is diffed again here; lines are only kept,
//! dropped, or kept as they are on both sides, and the hunk headers counted again to match.
//!
//! The patch is read and built as bytes, so a file that isn't UTF-8 goes back to git as it came.
//!
//! Ported from `lines.ts`.

use super::schema::{LineRange, LineSelection};
use crate::core::errors::{Error, Result, StageAction};
use crate::features::diff::hunk_header::parse_hunk_header_bytes;

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

/// What a line of a hunk is, as its marker says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineKind {
    /// ` `: on both sides.
    Context,
    /// `-`: only on the old side.
    Removed,
    /// `+`: only on the new side.
    Added,
}

impl LineKind {
    /// The kind `marker` says; `None` for another byte.
    fn of(marker: u8) -> Option<LineKind> {
        match marker {
            b' ' => Some(LineKind::Context),
            b'-' => Some(LineKind::Removed),
            b'+' => Some(LineKind::Added),
            _ => None,
        }
    }

    fn marker(self) -> u8 {
        match self {
            LineKind::Context => b' ',
            LineKind::Removed => b'-',
            LineKind::Added => b'+',
        }
    }

    /// Whether a line of this kind is on the old side.
    fn is_old(self) -> bool {
        self != LineKind::Added
    }

    /// Whether a line of this kind is on the new side.
    fn is_new(self) -> bool {
        self != LineKind::Removed
    }
}

/// One line of a hunk, without its marker; `text` keeps a CR before the line's end, if it has one.
#[derive(Debug, Clone, Copy)]
struct PatchLine<'a> {
    kind: LineKind,
    text: &'a [u8],
    /// It's the last of its side of the file, without a newline ("\ No newline at end of file").
    no_eol: bool,
}

#[derive(Debug)]
struct Hunk<'a> {
    old_start: u64,
    old_count: u64,
    new_start: u64,
    new_count: u64,
    /// What follows the second `@@`, e.g. the function the hunk is in.
    heading: &'a [u8],
    lines: Vec<PatchLine<'a>>,
}

const NO_EOL: &[u8] = b"\\ No newline at end of file";

const DEV_NULL: &[u8] = b"/dev/null";

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
pub fn lines_patch(
    patch: &[u8],
    selection: &LineSelection,
    action: LineAction,
) -> Result<Option<Vec<u8>>> {
    let verb = action.verb();
    let (header, hunks) = parse_patch(patch, verb)?;
    let picked = build_hunks(&hunks, selection, action)?;
    if picked.is_empty() {
        return Ok(None);
    }

    let file = parse_header(&header, verb)?;
    // Whether the file is still added, or deleted, as a whole: all of its lines move.
    let added = file.minus == DEV_NULL && picked.iter().all(|hunk| hunk.old_count == 0);
    let deleted = file.plus == DEV_NULL && picked.iter().all(|hunk| hunk.new_count == 0);
    let has_rename = !file.rename.is_empty();
    // Unstaging lines leaves the rename staged: they're unstaged from the renamed file.
    let renamed = has_rename && action == LineAction::Stage;
    let plus = if file.plus == DEV_NULL && !deleted {
        other_side(file.minus, b"b/")?
    } else {
        file.plus.to_vec()
    };
    let minus = if (file.minus == DEV_NULL && !added) || (has_rename && !renamed) {
        other_side(&plus, b"a/")?
    } else {
        file.minus.to_vec()
    };

    let mut built = Vec::with_capacity(patch.len());
    if has_rename && !renamed {
        push_line(
            &mut built,
            &[
                b"diff --git ",
                without_tab(&minus),
                b" ",
                without_tab(&plus),
            ],
        );
    } else {
        push_line(&mut built, &[file.diff]);
    }
    if added && let Some(created) = file.created {
        push_line(&mut built, &[created]);
    }
    if deleted && let Some(deleted) = file.deleted {
        push_line(&mut built, &[deleted]);
    }
    if renamed {
        for line in &file.rename {
            push_line(&mut built, &[line]);
        }
    }
    push_line(&mut built, &[b"--- ", &minus]);
    push_line(&mut built, &[b"+++ ", &plus]);
    for hunk in &picked {
        let old = range(hunk.old_start, hunk.old_count);
        let new = range(hunk.new_start, hunk.new_count);
        push_line(
            &mut built,
            &[
                b"@@ -",
                old.as_bytes(),
                b" +",
                new.as_bytes(),
                b" @@",
                hunk.heading,
            ],
        );
        for line in &hunk.lines {
            push_line(&mut built, &[&[line.kind.marker()], line.text]);
            if line.no_eol {
                push_line(&mut built, &[NO_EOL]);
            }
        }
    }
    Ok(Some(built))
}

/// Adds a line made of `parts` to `patch`, ended by a newline.
fn push_line(patch: &mut Vec<u8>, parts: &[&[u8]]) {
    for part in parts {
        patch.extend_from_slice(part);
    }
    patch.push(b'\n');
}

/// A hunk header's range, as git writes it: without the count when it's 1.
fn range(start: u64, count: u64) -> String {
    if count == 1 {
        start.to_string()
    } else {
        format!("{start},{count}")
    }
}

/// A line of a patch that isn't what git writes there, for a message: its first 40 characters.
fn unexpected_line(line: &[u8]) -> Error {
    let start: String = String::from_utf8_lossy(line).chars().take(40).collect();
    Error::Bug(format!("Unexpected line in a hunk: {start}"))
}

/// A hunk header's number; one too large to count is a patch git didn't write.
fn number(value: usize) -> Result<u64> {
    u64::try_from(value)
        .map_err(|_| Error::Bug(format!("Unexpected number in a hunk header: {value}")))
}

/// Splits `patch` into its header's lines and its hunks.
fn parse_patch(patch: &[u8], verb: StageAction) -> Result<(Vec<&[u8]>, Vec<Hunk<'_>>)> {
    let mut lines: Vec<&[u8]> = patch.split(|&byte| byte == b'\n').collect();
    // The newline that ends the last line.
    if lines.last().is_some_and(|last| last.is_empty()) {
        lines.pop();
    }
    let mut i = 0;
    let mut header = Vec::new();
    while i < lines.len() && !lines[i].starts_with(b"@@ ") {
        header.push(lines[i]);
        i += 1;
    }
    if header
        .iter()
        .any(|line| line.starts_with(b"Binary files ") || *line == b"GIT binary patch")
    {
        return Err(Error::LinesNotStageable(format!(
            "A binary file's changes can only be {verb} whole."
        )));
    }

    let mut hunks = Vec::new();
    while i < lines.len() {
        // A second file, after the first one's hunks: git has a file that changed type (from a
        // link to a file, say) as one deleted and another added.
        let Some(header) = parse_hunk_header_bytes(lines[i]) else {
            return Err(Error::LinesNotStageable(format!(
                "A file that changed type can only be {verb} whole."
            )));
        };
        i += 1;
        let mut hunk = Hunk {
            old_start: number(header.old_start)?,
            old_count: number(header.old_count)?,
            new_start: number(header.new_start)?,
            new_count: number(header.new_count)?,
            heading: header.heading,
            lines: Vec::new(),
        };
        // Read by the header's counts: with `diff.suppressBlankEmpty`, an empty unchanged line is
        // left empty, without its marker.
        let mut old_left = hunk.old_count;
        let mut new_left = hunk.new_count;
        while i < lines.len() && (old_left > 0 || new_left > 0 || lines[i].starts_with(b"\\")) {
            let line = lines[i];
            i += 1;
            let (marker, text) = line
                .split_first()
                .map_or((b' ', &b""[..]), |(marker, text)| (*marker, text));
            if marker == b'\\' {
                if let Some(last) = hunk.lines.last_mut() {
                    last.no_eol = true;
                }
                continue;
            }
            let Some(kind) = LineKind::of(marker) else {
                return Err(unexpected_line(line));
            };
            // More lines of a side than the header counts is a patch git didn't write.
            if kind.is_old() {
                old_left = old_left
                    .checked_sub(1)
                    .ok_or_else(|| unexpected_line(line))?;
            }
            if kind.is_new() {
                new_left = new_left
                    .checked_sub(1)
                    .ok_or_else(|| unexpected_line(line))?;
            }
            hunk.lines.push(PatchLine {
                kind,
                text,
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
    diff: &'a [u8],
    /// The paths on its `---` and `+++` lines, as git wrote them, or `/dev/null`.
    minus: &'a [u8],
    plus: &'a [u8],
    /// Its `new file mode` or `deleted file mode` line, for a file added or deleted.
    created: Option<&'a [u8]>,
    deleted: Option<&'a [u8]>,
    /// Its `rename from` and `rename to` lines, for a renamed file.
    rename: Vec<&'a [u8]>,
}

/// The lines of a patch's header that end in a mode: the `index` line, and the ones that say a
/// mode changed, or the file's mode when it's added or deleted. Not the others, which have paths
/// that could end like one.
const MODE_LINES: [&[u8]; 5] = [
    b"index ",
    b"old mode ",
    b"new mode ",
    b"new file mode ",
    b"deleted file mode ",
];

/// Reads the parts of a patch's header that are kept. The `index` line isn't: its object names are
/// of the whole file, not of the lines picked (git doesn't check them in a patch of text), and the
/// mode on it, like the `old mode` and `new mode` of a mode change, would move the mode too.
fn parse_header<'a>(header: &[&'a [u8]], verb: StageAction) -> Result<FileHeader<'a>> {
    let mut file = FileHeader {
        diff: header.first().copied().unwrap_or_default(),
        minus: b"",
        plus: b"",
        created: None,
        deleted: None,
        rename: Vec::new(),
    };
    for &line in header {
        // A link's contents are the path it points to, and a submodule's the commit it's at.
        if MODE_LINES.iter().any(|start| line.starts_with(start)) {
            let link = line.ends_with(b" 120000");
            if link || line.ends_with(b" 160000") {
                let what = if link { "A link" } else { "A submodule" };
                return Err(Error::LinesNotStageable(format!(
                    "{what} can only be {verb} whole."
                )));
            }
        }
        if let Some(path) = line.strip_prefix(b"--- ") {
            file.minus = path;
        } else if let Some(path) = line.strip_prefix(b"+++ ") {
            file.plus = path;
        } else if line.starts_with(b"new file mode ") {
            file.created = Some(line);
        } else if line.starts_with(b"deleted file mode ") {
            file.deleted = Some(line);
        } else if line.starts_with(b"rename from ") || line.starts_with(b"rename to ") {
            file.rename.push(line);
        }
    }
    if !file.diff.starts_with(b"diff --git ") || file.minus.is_empty() || file.plus.is_empty() {
        return Err(Error::Bug("The patch has no file header.".to_owned()));
    }
    Ok(file)
}

/// `path` from a `---` or `+++` line (`a/x` or `b/x`, or quoted, `"a/x\ty"`), as the other side has
/// it: with `prefix`.
fn other_side(path: &[u8], prefix: &[u8]) -> Result<Vec<u8>> {
    let (quote, name) = match path.strip_prefix(b"\"") {
        Some(name) => (&b"\""[..], name),
        None => (&b""[..], path),
    };
    let Some(rest) = name
        .strip_prefix(b"a/")
        .or_else(|| name.strip_prefix(b"b/"))
    else {
        return Err(Error::Bug(format!(
            "Unexpected path in a patch: {}",
            String::from_utf8_lossy(path)
        )));
    };
    Ok([quote, prefix, rest].concat())
}

/// A path from a `---` or `+++` line without the tab git ends one with that has a space, as on the
/// `diff --git` line.
fn without_tab(path: &[u8]) -> &[u8] {
    path.strip_suffix(b"\t").unwrap_or(path)
}

/// How many of `lines` are on a side, as `on_side` says of their kinds.
fn side_count(lines: &[PatchLine<'_>], on_side: fn(LineKind) -> bool) -> u64 {
    lines
        .iter()
        .filter(|line| on_side(line.kind))
        .fold(0, |count, _| count + 1)
}

/// The hunks with the lines picked, renumbered, without the ones left with no changes.
fn build_hunks<'a>(
    hunks: &[Hunk<'a>],
    selection: &LineSelection,
    action: LineAction,
) -> Result<Vec<Hunk<'a>>> {
    let mut deletions = Picked::new(&selection.deletions);
    let mut additions = Picked::new(&selection.additions);
    let mut built = Vec::new();
    // How many lines the old and the new side of the hunks built so far have: the new side has
    // moved by the difference.
    let (mut old_lines, mut new_lines) = (0_u64, 0_u64);
    for hunk in hunks {
        let mut lines = Vec::new();
        let mut old_line = hunk.old_start;
        let mut new_line = hunk.new_start;
        for &line in &hunk.lines {
            let unchanged = PatchLine {
                kind: LineKind::Context,
                ..line
            };
            match line.kind {
                LineKind::Context => {
                    old_line += 1;
                    new_line += 1;
                    lines.push(line);
                }
                LineKind::Removed => {
                    let picked = deletions.has(old_line);
                    old_line += 1;
                    if picked {
                        lines.push(line);
                    } else if action == LineAction::Stage {
                        lines.push(unchanged);
                    }
                }
                LineKind::Added => {
                    let picked = additions.has(new_line);
                    new_line += 1;
                    if picked {
                        lines.push(line);
                    } else if action == LineAction::Unstage {
                        lines.push(unchanged);
                    }
                }
            }
        }
        if lines.iter().all(|line| line.kind == LineKind::Context) {
            continue;
        }

        let ended = end_of_file(&lines);
        let old_count = side_count(&ended, LineKind::is_old);
        let new_count = side_count(&ended, LineKind::is_new);
        // The side the patch is applied to keeps its lines, and their numbers; the other side's are
        // moved by the lines the hunks before added or removed. A side without lines in the hunk is
        // numbered by the line before it. Only a patch git didn't write numbers one before 0.
        let moved = |start: u64, count: u64, plus: u64, minus: u64| {
            start
                .checked_sub(u64::from(count > 0))
                .and_then(|before| before.checked_add(plus))
                .and_then(|before| before.checked_sub(minus))
                .ok_or_else(|| Error::Bug("Unexpected hunk numbers in a patch.".to_owned()))
        };
        let (old_start, new_start) = if action == LineAction::Stage {
            let before = moved(hunk.old_start, hunk.old_count, new_lines, old_lines)?;
            (hunk.old_start, before + u64::from(new_count > 0))
        } else {
            let before = moved(hunk.new_start, hunk.new_count, old_lines, new_lines)?;
            (before + u64::from(old_count > 0), hunk.new_start)
        };
        old_lines += old_count;
        new_lines += new_count;
        built.push(Hunk {
            old_start,
            old_count,
            new_start,
            new_count,
            heading: hunk.heading,
            lines: ended,
        });
    }
    Ok(built)
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
        let old_after = after.iter().any(|next| next.kind.is_old());
        let new_after = after.iter().any(|next| next.kind.is_new());
        match line.kind {
            LineKind::Removed => ended.push(PatchLine {
                no_eol: !old_after,
                ..line
            }),
            LineKind::Added => ended.push(PatchLine {
                no_eol: !new_after,
                ..line
            }),
            LineKind::Context if old_after == new_after => ended.push(PatchLine {
                no_eol: !old_after,
                ..line
            }),
            LineKind::Context => {
                ended.push(PatchLine {
                    kind: LineKind::Removed,
                    text: line.text,
                    no_eol: !old_after,
                });
                ended.push(PatchLine {
                    kind: LineKind::Added,
                    text: line.text,
                    no_eol: !new_after,
                });
            }
        }
    }
    ended
}

/// Whether a line is in some ranges, for line numbers asked in increasing order: in one pass over
/// the ranges, however many lines and ranges there are.
struct Picked {
    /// `(start, end)`, by their start.
    ranges: Vec<(u64, u64)>,
    next: usize,
}

impl Picked {
    fn new(ranges: &[LineRange]) -> Self {
        let mut ranges: Vec<(u64, u64)> = ranges
            .iter()
            .map(|range| (range.start(), range.end()))
            .collect();
        ranges.sort_by_key(|&(start, _)| start);
        Picked { ranges, next: 0 }
    }

    fn has(&mut self, line: u64) -> bool {
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

    /// The patch that moves the lines `selection` picks from `patch`, as text.
    fn built(patch: &str, selection: LineSelection, action: LineAction) -> Option<String> {
        lines_patch(patch.as_bytes(), &selection, action)
            .unwrap()
            .map(|built| String::from_utf8(built).unwrap())
    }

    fn stage(patch: &str, selection: LineSelection) -> Option<String> {
        built(patch, selection, LineAction::Stage)
    }

    fn unstage(patch: &str, selection: LineSelection) -> Option<String> {
        built(patch, selection, LineAction::Unstage)
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
    fn keeps_the_bytes_of_a_file_that_isnt_utf8_as_they_are() {
        let latin1 = b"diff --git a/f.txt b/f.txt\n--- a/f.txt\n+++ b/f.txt\n@@ -1 +1,3 @@ caf\xe9\n \xe9\n+\xff\xfe\n+b\n";
        assert_eq!(
            lines_patch(latin1, &pick(&[], &[2]), LineAction::Stage).unwrap(),
            Some(
                b"diff --git a/f.txt b/f.txt\n--- a/f.txt\n+++ b/f.txt\n@@ -1 +1,2 @@ caf\xe9\n \xe9\n+\xff\xfe\n"
                    .to_vec()
            )
        );
    }

    #[test]
    fn reads_a_heading_with_any_characters_in_it() {
        let heading = with(&HEADER, &["@@ -1 +1 @@ a\u{2028}b\rc", "-a", "+b"]);
        assert_eq!(
            stage(&heading, pick(&[1], &[1])),
            Some(with(&BUILT, &["@@ -1 +1 @@ a\u{2028}b\rc", "-a", "+b"]))
        );
    }

    #[test]
    fn fails_for_more_lines_than_a_hunk_header_counts() {
        // Two removed lines where the header has one: not a patch git wrote.
        let miscounted = with(&HEADER, &["@@ -1 +1,2 @@", "-a", "-b", "+c"]);
        let error =
            lines_patch(miscounted.as_bytes(), &pick(&[1], &[1]), LineAction::Stage).unwrap_err();
        assert!(matches!(error, Error::Bug(_)), "{error:?}");
        assert_eq!(error.to_string(), "Unexpected line in a hunk: -b");
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
                built(&chmod, pick(&[1], &[1]), action),
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
            match lines_patch(input.as_bytes(), &pick(&[1], &[1]), action) {
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
    fn stages_lines_of_a_file_whose_name_ends_like_a_links_mode() {
        let named = patch(&[
            "diff --git a/notes 120000 b/notes 120000",
            "index 1111111..2222222 100644",
            "--- a/notes 120000\t",
            "+++ b/notes 120000\t",
            "@@ -1 +1 @@",
            "-a",
            "+b",
        ]);
        assert_eq!(
            stage(&named, pick(&[1], &[1])),
            Some(patch(&[
                "diff --git a/notes 120000 b/notes 120000",
                "--- a/notes 120000\t",
                "+++ b/notes 120000\t",
                "@@ -1 +1 @@",
                "-a",
                "+b",
            ]))
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
