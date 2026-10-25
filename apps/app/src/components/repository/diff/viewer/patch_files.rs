//! Fitting whole files to a patch git gave. Ported from `patch-files.ts`.

use std::collections::BTreeMap;

use super::diff_metadata::{FileDiff, HunkContent, Lines, Side, end_boundary, start_boundary};

/// The unchanged lines shown around a hunk, from a gap before it (by the hunk's index; the gap
/// after the last hunk is the number of hunks): `from_start` lines at the top of the gap, below
/// the hunk before, and `from_end` at its bottom, above the hunk (`HunkExpansionRegion`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Expansion {
    pub from_start: usize,
    pub from_end: usize,
}

/// The lines shown around each hunk, by the index of the gap they're from.
pub type Expanded = BTreeMap<usize, Expansion>;

/// `contents`, if they're the file `diff` is a patch of on its `side`: each of its hunks' lines is
/// where the patch has it. A file in the working tree can have changed since its patch was read,
/// and one with CRLF line ends throughout is compared with LF ones (git's `core.autocrlf`), which
/// the contents are changed to then (see `as_lf`). Fails if they aren't.
pub fn fit_to_patch(diff: &FileDiff, side: Side, contents: &str) -> Result<String, String> {
    if has_patch_lines(diff, side, contents) {
        return Ok(contents.to_owned());
    }
    if let Some(lf) = as_lf(contents)
        && has_patch_lines(diff, side, &lf)
    {
        return Ok(lf);
    }
    Err(changed_error(contents))
}

/// `contents` with LF line ends, if they have CRLF ones throughout, which saving them makes CRLF
/// again (see `save_working_tree_file`); `None` otherwise. A file that mixes them isn't taken
/// with LF ones: its line ends couldn't be told apart again, and saving it would change them all.
fn as_lf(contents: &str) -> Option<String> {
    if !contents.contains("\r\n") || has_bare_lf(contents) {
        return None;
    }
    Some(contents.replace("\r\n", "\n"))
}

/// Whether `contents` has a line end that's LF alone.
fn has_bare_lf(contents: &str) -> bool {
    let bytes = contents.as_bytes();
    bytes
        .iter()
        .enumerate()
        .any(|(ix, byte)| *byte == b'\n' && (ix == 0 || bytes[ix - 1] != b'\r'))
}

fn changed_error(contents: &str) -> String {
    if contents.contains("\r\n") && has_bare_lf(contents) {
        "This file mixes CRLF and LF line ends, which git shows as all LF here.".to_owned()
    } else {
        "The file has changed since its changes were loaded. Try again in a moment.".to_owned()
    }
}

/// `contents`, if they're all of `lines`, the new side of a patch that has the whole file (an
/// added one), with LF line ends for CRLF ones as [`fit_to_patch`] takes them. Fails if they
/// aren't.
pub fn fit_to_lines(lines: &Lines, contents: &str) -> Result<String, String> {
    let whole = lines.text();
    if contents == whole {
        return Ok(contents.to_owned());
    }
    if as_lf(contents).as_deref() == Some(whole) {
        return Ok(whole.to_owned());
    }
    Err(changed_error(contents))
}

/// Whether `contents` have the lines of each of `diff`'s hunks on its `side`, where it has them.
fn has_patch_lines(diff: &FileDiff, side: Side, contents: &str) -> bool {
    let patch_lines = diff.lines(side);
    // The line `line` (from 0) of `contents` starts at `offset`.
    let mut line = 0;
    let mut offset = 0;
    for hunk in &diff.hunks {
        let (start, count, mut index) = match side {
            Side::Deletions => (
                hunk.deletion_start,
                hunk.deletion_count,
                hunk.deletion_line_index,
            ),
            Side::Additions => (
                hunk.addition_start,
                hunk.addition_count,
                hunk.addition_line_index,
            ),
        };
        if count == 0 {
            continue;
        }
        while line + 1 < start {
            let Some(end) = contents[offset..].find('\n') else {
                return false;
            };
            offset += end + 1;
            line += 1;
        }
        let last = index + count;
        while index < last {
            // A line keeps its newline, as in the patch; the last one may have none.
            let next = contents[offset..]
                .find('\n')
                .map_or(contents.len(), |end| offset + end + 1);
            if offset == next || patch_lines.get(index) != Some(&contents[offset..next]) {
                return false;
            }
            offset = next;
            index += 1;
            line += 1;
        }
    }
    true
}

/// Whether the hydrated `diff` (see `hydrate`) has as many unchanged lines after its last hunk in
/// both files, as the viewer takes for granted: a file in the working tree that got lines at its
/// end since its patch was read doesn't.
pub fn has_matching_ends(diff: &FileDiff) -> bool {
    let Some(last) = diff.hunks.last() else {
        return true;
    };
    let additions = diff.addition_lines.len() as isize
        - end_boundary(last.addition_start, last.addition_count) as isize;
    let deletions = diff.deletion_lines.len() as isize
        - end_boundary(last.deletion_start, last.deletion_count) as isize;
    additions == deletions
}

/// The unchanged lines that were shown around `from`'s hunks (`expanded`, by hunk, as the viewer
/// keeps them), around `to`'s instead: both hydrated diffs of the same file, `to` with changes
/// made since. They're found by their lines in the old file, which a change to the new one leaves
/// alone: the index, or HEAD, for uncommitted changes.
pub fn carried_expansion(from: &FileDiff, expanded: &Expanded, to: &FileDiff) -> Expanded {
    // The old file's lines that were shown, as [start, end) ranges.
    let mut shown = Vec::new();
    for gap in collapsed_gaps(from) {
        let Some(region) = expanded.get(&gap.index) else {
            continue;
        };
        if region.from_start > 0 {
            shown.push((gap.start, gap.start + region.from_start));
        }
        if region.from_end > 0 {
            shown.push((gap.end.saturating_sub(region.from_end), gap.end));
        }
    }
    let mut carried = Expanded::new();
    if shown.is_empty() {
        return carried;
    }
    for gap in collapsed_gaps(to) {
        let mut from_start = 0;
        let mut from_end = 0;
        for &(start, end) in &shown {
            if end <= gap.start || start >= gap.end {
                continue;
            }
            if start <= gap.start {
                from_start = from_start.max(end.min(gap.end) - gap.start);
            }
            if end >= gap.end {
                from_end = from_end.max(gap.end - start.max(gap.start));
            }
        }
        if from_start > 0 || from_end > 0 {
            carried.insert(
                gap.index,
                Expansion {
                    from_start,
                    from_end,
                },
            );
        }
    }
    carried
}

/// Unchanged lines hidden before one of a diff's hunks, or after the last, as [start, end) in the
/// old file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gap {
    pub index: usize,
    pub start: usize,
    pub end: usize,
}

/// The unchanged lines hidden before each of the hydrated `diff`'s hunks, and after the last (as
/// hunk `hunks.len()`, as the viewer has it), as [start, end) in the old file.
pub fn collapsed_gaps(diff: &FileDiff) -> Vec<Gap> {
    let mut gaps: Vec<Gap> = diff
        .hunks
        .iter()
        .enumerate()
        .map(|(index, hunk)| {
            let end = start_boundary(hunk.deletion_start, hunk.deletion_count);
            Gap {
                index,
                start: end.saturating_sub(hunk.collapsed_before),
                end,
            }
        })
        .collect();
    if let Some(last) = diff.hunks.last() {
        gaps.push(Gap {
            index: diff.hunks.len(),
            start: end_boundary(last.deletion_start, last.deletion_count),
            end: diff.deletion_lines.len(),
        });
    }
    gaps.retain(|gap| gap.end > gap.start);
    gaps
}

/// The lines of a run of a hunk's content on `side`, as indexes into the diff's lines.
pub fn content_lines(content: &HunkContent, side: Side) -> std::ops::Range<usize> {
    match (*content, side) {
        (
            HunkContent::Context {
                lines,
                deletion_line_index: start,
                ..
            },
            Side::Deletions,
        )
        | (
            HunkContent::Context {
                lines,
                addition_line_index: start,
                ..
            },
            Side::Additions,
        ) => start..start + lines,
        (
            HunkContent::Change {
                deletions: lines,
                deletion_line_index: start,
                ..
            },
            Side::Deletions,
        )
        | (
            HunkContent::Change {
                additions: lines,
                addition_line_index: start,
                ..
            },
            Side::Additions,
        ) => start..start + lines,
    }
}

#[cfg(test)]
mod tests {
    use super::super::diff_metadata::{LoadedSide, hydrate, parse_patch_files};
    use super::*;

    /// A file of `count` numbered lines, with `changes` made to some of them.
    fn lines(count: usize, changes: &[(usize, &str)]) -> String {
        (1..=count)
            .map(|line| {
                let text = changes
                    .iter()
                    .find(|(number, _)| *number == line)
                    .map_or_else(|| format!("line {line}"), |(_, text)| (*text).to_owned());
                format!("{text}\n")
            })
            .collect()
    }

    /// The patch of a change from `before` to `after`, as git would give it, parsed.
    fn patch_of(hunks: &str) -> FileDiff {
        let patch = [
            "diff --git a/f.txt b/f.txt",
            "index 1111111111111111111111111111111111111111..2222222222222222222222222222222222222222 100644",
            "--- a/f.txt",
            "+++ b/f.txt",
            hunks,
        ]
        .join("\n");
        parse_patch_files(&patch, Some("test")).remove(0)
    }

    /// Line 10 of 40 changed, with 3 lines of context.
    fn changed10() -> FileDiff {
        patch_of(
            "@@ -7,7 +7,7 @@\n line 7\n line 8\n line 9\n-line 10\n+ten\n line 11\n line 12\n line 13\n",
        )
    }

    /// `diff` filled in with the whole of a file of 40 lines, and `after`.
    fn hydrated(diff: &FileDiff, after: &str) -> FileDiff {
        let before = lines(40, &[]);
        hydrate(
            diff,
            Some(LoadedSide {
                contents: &before,
                cache_key: None,
            }),
            Some(LoadedSide {
                contents: after,
                cache_key: None,
            }),
        )
        .unwrap()
    }

    // fitToPatch

    #[test]
    fn takes_the_file_the_patch_is_of_on_either_side() {
        let diff = changed10();
        assert_eq!(
            fit_to_patch(&diff, Side::Deletions, &lines(40, &[])).unwrap(),
            lines(40, &[])
        );
        let after = lines(40, &[(10, "ten")]);
        assert_eq!(fit_to_patch(&diff, Side::Additions, &after).unwrap(), after);
    }

    #[test]
    fn refuses_a_file_that_has_changed_since() {
        let diff = changed10();
        let error = fit_to_patch(&diff, Side::Additions, &lines(40, &[(10, "TEN")])).unwrap_err();
        assert!(error.contains("has changed"), "{error}");
        let error = fit_to_patch(&diff, Side::Additions, &lines(11, &[(10, "ten")])).unwrap_err();
        assert!(error.contains("has changed"), "{error}");
    }

    #[test]
    fn takes_a_file_with_crlf_line_ends_for_a_patch_with_lf_ones_as_lf() {
        let crlf = lines(40, &[(10, "ten")]).replace('\n', "\r\n");
        assert_eq!(
            fit_to_patch(&changed10(), Side::Additions, &crlf).unwrap(),
            lines(40, &[(10, "ten")])
        );
    }

    #[test]
    fn doesnt_take_a_file_mixing_crlf_and_lf_for_a_patch_with_lf_ones_which_saving_would_change() {
        // CRLF where the changes are (as `core.autocrlf` has git show them with LF), LF at the end.
        let crlf = lines(40, &[(10, "ten")]).replace('\n', "\r\n");
        let mixed = format!("{}\n", crlf.strip_suffix("\r\n").unwrap());
        let error = fit_to_patch(&changed10(), Side::Additions, &mixed).unwrap_err();
        assert!(error.contains("mixes CRLF and LF"), "{error}");
    }

    // fitToLines

    #[test]
    fn takes_an_added_file_thats_all_of_the_patchs_lines_with_lf_line_ends_for_crlf_ones() {
        let mut two = Lines::new();
        two.push("a\n");
        two.push("b");
        assert_eq!(fit_to_lines(&two, "a\nb").unwrap(), "a\nb");
        let mut ended = Lines::new();
        ended.push("a\n");
        ended.push("b\n");
        assert_eq!(fit_to_lines(&ended, "a\r\nb\r\n").unwrap(), "a\nb\n");
        let mut one = Lines::new();
        one.push("a\n");
        assert!(
            fit_to_lines(&one, "a\nmore\n")
                .unwrap_err()
                .contains("has changed")
        );
    }

    // hasMatchingEnds

    #[test]
    fn tells_a_file_that_got_lines_at_its_end_since_its_patch_was_read() {
        let diff = changed10();
        assert!(has_matching_ends(&hydrated(
            &diff,
            &lines(40, &[(10, "ten")])
        )));
        assert!(!has_matching_ends(&hydrated(
            &diff,
            &lines(41, &[(10, "ten")])
        )));
    }

    // carriedExpansion

    #[test]
    fn keeps_the_lines_shown_around_the_changes_by_their_place_in_the_old_file() {
        let from = hydrated(&changed10(), &lines(40, &[(10, "ten")]));
        // Lines 1-2 above the hunk and 14-18 below it (the lines after the last hunk are its own).
        let expanded = Expanded::from([
            (
                0,
                Expansion {
                    from_start: 2,
                    from_end: 0,
                },
            ),
            (
                1,
                Expansion {
                    from_start: 5,
                    from_end: 0,
                },
            ),
        ]);
        // Line 30 changed too since: the lines below the first hunk are now split by another.
        let to = hydrated(
            &patch_of(
                &[
                    "@@ -7,7 +7,7 @@\n line 7\n line 8\n line 9\n-line 10\n+ten\n line 11\n line 12\n line 13",
                    "@@ -27,7 +27,7 @@\n line 27\n line 28\n line 29\n-line 30\n+thirty\n line 31\n line 32\n line 33\n",
                ]
                .join("\n"),
            ),
            &lines(40, &[(10, "ten"), (30, "thirty")]),
        );
        assert_eq!(
            carried_expansion(&from, &expanded, &to),
            Expanded::from([
                (
                    0,
                    Expansion {
                        from_start: 2,
                        from_end: 0
                    }
                ),
                (
                    1,
                    Expansion {
                        from_start: 5,
                        from_end: 0
                    }
                ),
            ])
        );
    }
}
