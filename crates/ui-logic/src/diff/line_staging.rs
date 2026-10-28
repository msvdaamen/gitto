//! Which changed lines of a diff on show are picked to stage or unstage: from a selection of its
//! rows, or a hunk, and where the keyboard moves a selection to. Read from the diff the viewer
//! parsed from git's patch, by the same line numbers the patch's hunk headers count. Ported from
//! `line-staging.ts`.

use gitto_git::features::staging::schema::{LineRange, LineSelection};

use super::diff_metadata::{FileDiff, HunkContent, Side};

/// A removed or added line of a diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChangedLine {
    /// Removed lines are on the deletions side, numbered in the old file; added ones in the new.
    pub side: Side,
    pub line_number: usize,
    /// The index of its hunk.
    pub hunk: usize,
    /// Where it is in the old and the new file: a removed line is before the new side's line that
    /// follows it, an added one before the old side's.
    pub old: usize,
    pub new: usize,
}

impl ChangedLine {
    /// Where it is in the file on `side` (see [`old`](Self::old)).
    pub fn position(&self, side: Side) -> usize {
        match side {
            Side::Deletions => self.old,
            Side::Additions => self.new,
        }
    }
}

/// Every removed and added line of `diff`, in the order they're in: by hunk, removed ones first.
pub fn changed_lines(diff: &FileDiff) -> Vec<ChangedLine> {
    let mut lines = Vec::new();
    for (index, hunk) in diff.hunks.iter().enumerate() {
        // A side without lines in the hunk is numbered by the line before it.
        let mut old = if hunk.deletion_count > 0 {
            hunk.deletion_start
        } else {
            hunk.deletion_start + 1
        };
        let mut next = if hunk.addition_count > 0 {
            hunk.addition_start
        } else {
            hunk.addition_start + 1
        };
        for content in &hunk.content {
            match *content {
                HunkContent::Context { lines: count, .. } => {
                    old += count;
                    next += count;
                }
                HunkContent::Change {
                    deletions,
                    additions,
                    ..
                } => {
                    for i in 0..deletions {
                        lines.push(ChangedLine {
                            side: Side::Deletions,
                            line_number: old + i,
                            hunk: index,
                            old: old + i,
                            new: next,
                        });
                    }
                    let after = old + deletions;
                    for i in 0..additions {
                        lines.push(ChangedLine {
                            side: Side::Additions,
                            line_number: next + i,
                            hunk: index,
                            old: after,
                            new: next + i,
                        });
                    }
                    old = after;
                    next += additions;
                }
            }
        }
    }
    lines
}

/// Rows from one line's to another's, by their sides and numbers (`SelectedLineRange`): in either
/// direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectedRange {
    pub start: usize,
    pub side: Side,
    pub end: usize,
    pub end_side: Side,
}

impl SelectedRange {
    /// The rows from `start` to `end`, both on `side`.
    pub fn new(start: usize, side: Side, end: usize) -> Self {
        SelectedRange {
            start,
            side,
            end,
            end_side: side,
        }
    }
}

/// The changed lines in the rows a selection spans, in either direction. `row_of` is the row a
/// line is on in the view, unified or split; `None` if it isn't on one.
pub fn selected_lines(
    lines: &[ChangedLine],
    range: &SelectedRange,
    row_of: &dyn Fn(usize, Side) -> Option<usize>,
) -> Vec<ChangedLine> {
    let (Some(start), Some(end)) = (
        row_of(range.start, range.side),
        row_of(range.end, range.end_side),
    ) else {
        return Vec::new();
    };
    let (top, bottom) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };
    lines
        .iter()
        .filter(|line| {
            row_of(line.line_number, line.side).is_some_and(|row| row >= top && row <= bottom)
        })
        .copied()
        .collect()
}

/// `lines` as the ranges of line numbers on each side that the git crate takes.
pub fn to_selection(lines: &[ChangedLine]) -> LineSelection {
    let ranges = |side: Side| {
        let mut numbers: Vec<usize> = lines
            .iter()
            .filter(|line| line.side == side)
            .map(|line| line.line_number)
            .collect();
        numbers.sort_unstable();
        let mut result: Vec<(usize, usize)> = Vec::new();
        for number in numbers {
            match result.last_mut() {
                Some(last) if number <= last.1 + 1 => last.1 = last.1.max(number),
                _ => result.push((number, number)),
            }
        }
        result
            .into_iter()
            .filter_map(|(start, end)| LineRange::new(start as u64, end as u64).ok())
            .collect()
    };
    LineSelection {
        deletions: ranges(Side::Deletions),
        additions: ranges(Side::Additions),
    }
}

/// A row with changed lines, and the first of its lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChangedRow {
    pub row: usize,
    pub line: ChangedLine,
}

/// Rows with changed lines, in order, each with the first of its lines: in split view a row can
/// have a removed line and an added one, side by side. What the keyboard moves through.
pub fn changed_rows(
    lines: &[ChangedLine],
    row_of: &dyn Fn(usize, Side) -> Option<usize>,
) -> Vec<ChangedRow> {
    let mut rows: Vec<ChangedRow> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in lines {
        if let Some(row) = row_of(line.line_number, line.side)
            && seen.insert(row)
        {
            rows.push(ChangedRow { row, line: *line });
        }
    }
    rows.sort_by_key(|row| row.row);
    rows
}

/// The changed lines of a diff laid out in rows, by row (see `viewer_rows::Layout`): worked out
/// once per layout, so that moving the selection, dragging it, or pointing at a line looks the
/// lines up rather than going through all of them, which a new file of tens of thousands of lines
/// has.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangedRows {
    /// Every changed line that's on a row, by row: side by side, a row can have two. With where
    /// each is in [`changed_lines`]' order.
    lines: Vec<(ChangedRow, usize)>,
    /// The rows with changed lines, each with the first of its lines (see [`changed_rows`]).
    rows: Vec<ChangedRow>,
}

impl ChangedRows {
    /// `lines` (see [`changed_lines`]) by the rows they're on: `row_of` is the row a line is on.
    pub fn new(lines: &[ChangedLine], row_of: &dyn Fn(usize, Side) -> Option<usize>) -> Self {
        let mut by_row: Vec<(ChangedRow, usize)> = lines
            .iter()
            .enumerate()
            .filter_map(|(ix, line)| {
                let row = row_of(line.line_number, line.side)?;
                Some((ChangedRow { row, line: *line }, ix))
            })
            .collect();
        by_row.sort_by_key(|(row, ix)| (row.row, *ix));
        let mut rows: Vec<ChangedRow> = Vec::new();
        for (row, _) in &by_row {
            if rows.last().is_none_or(|last| last.row != row.row) {
                rows.push(*row);
            }
        }
        ChangedRows {
            lines: by_row,
            rows,
        }
    }

    /// The rows with changed lines, in order, each with the first of its lines: what the keyboard
    /// moves through.
    pub fn rows(&self) -> &[ChangedRow] {
        &self.rows
    }

    /// The changed lines on the rows from `top` to `bottom`, in [`changed_lines`]' order.
    pub fn in_rows(&self, top: usize, bottom: usize) -> Vec<ChangedLine> {
        let start = self.lines.partition_point(|(row, _)| row.row < top);
        let end = self.lines.partition_point(|(row, _)| row.row <= bottom);
        let mut lines: Vec<(ChangedRow, usize)> = self.lines[start..end.max(start)].to_vec();
        lines.sort_by_key(|(_, ix)| *ix);
        lines.into_iter().map(|(row, _)| row.line).collect()
    }

    /// The changed line `number` of `side`, on the row `row`, if it's one.
    pub fn line_at(&self, row: usize, number: usize, side: Side) -> Option<ChangedLine> {
        let start = self.lines.partition_point(|(at, _)| at.row < row);
        self.lines[start..]
            .iter()
            .take_while(|(at, _)| at.row == row)
            .map(|(at, _)| at.line)
            .find(|line| line.side == side && line.line_number == number)
    }

    /// The changed lines of the hunk `hunk`, in [`changed_lines`]' order.
    pub fn hunk_lines(&self, hunk: usize) -> Vec<ChangedLine> {
        let mut lines: Vec<&(ChangedRow, usize)> = self
            .lines
            .iter()
            .filter(|(row, _)| row.line.hunk == hunk)
            .collect();
        lines.sort_by_key(|(_, ix)| *ix);
        lines.into_iter().map(|(row, _)| row.line).collect()
    }

    /// The rows of the hunk `hunk`: a run of [`rows`](Self::rows), as hunks are laid out in order.
    pub fn hunk_rows(&self, hunk: usize) -> &[ChangedRow] {
        let start = self.rows.partition_point(|row| row.line.hunk < hunk);
        let end = self.rows.partition_point(|row| row.line.hunk <= hunk);
        &self.rows[start..end.max(start)]
    }
}

/// A selection of the rows from `from`'s to `to`'s.
pub fn range_of(from: &ChangedLine, to: &ChangedLine) -> SelectedRange {
    SelectedRange {
        start: from.line_number,
        side: from.side,
        end: to.line_number,
        end_side: to.side,
    }
}

/// Where a hunk's button goes: after the line `line_number` of `side`, or above the file's first
/// line for line 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HunkButton {
    pub side: Side,
    pub line_number: usize,
    pub hunk: usize,
}

/// Where each hunk's button goes: below the unchanged line before its first change, which puts it
/// above the change; or above the first line of the file, for a hunk that starts with a change
/// (only at the top of the file, where there's no unchanged line before it).
pub fn hunk_buttons(diff: &FileDiff) -> Vec<HunkButton> {
    let mut buttons = Vec::new();
    for (hunk, data) in diff.hunks.iter().enumerate() {
        match data.content.first() {
            Some(HunkContent::Context { lines, .. }) if *lines > 0 => {
                let start = if data.addition_count > 0 {
                    data.addition_start
                } else {
                    data.addition_start + 1
                };
                buttons.push(HunkButton {
                    side: Side::Additions,
                    line_number: start + lines - 1,
                    hunk,
                });
            }
            Some(HunkContent::Change { additions, .. }) if hunk == 0 => {
                let side = if *additions > 0 {
                    Side::Additions
                } else {
                    Side::Deletions
                };
                buttons.push(HunkButton {
                    side,
                    line_number: 0,
                    hunk,
                });
            }
            _ => {}
        }
    }
    buttons
}

#[cfg(test)]
mod tests {
    use super::super::diff_metadata::parse_patch_files;
    use super::*;

    /// The rows lines are on, as the viewer numbers them, in unified or split view.
    fn rows(diff: &FileDiff, split: bool) -> impl Fn(usize, Side) -> Option<usize> + '_ {
        move |line_number, side| {
            diff.line_index(line_number, side)
                .map(|(unified, split_row)| if split { split_row } else { unified })
        }
    }

    fn parse(lines: &[&str]) -> FileDiff {
        parse_patch_files(&format!("{}\n", lines.join("\n")), Some("test")).remove(0)
    }

    const HEADER: [&str; 3] = ["diff --git a/f.txt b/f.txt", "--- a/f.txt", "+++ b/f.txt"];

    /// Two hunks: in the first, 3 replaced by two lines; in the second, 12 removed.
    fn diff() -> FileDiff {
        let mut lines = HEADER.to_vec();
        lines.extend([
            "@@ -1,5 +1,6 @@",
            " 1",
            " 2",
            "-3",
            "+three",
            "+3b",
            " 4",
            " 5",
            "@@ -10,5 +11,4 @@",
            " 10",
            " 11",
            "-12",
            " 13",
            " 14",
        ]);
        parse(&lines)
    }

    fn changed(side: Side, line_number: usize, hunk: usize, old: usize, new: usize) -> ChangedLine {
        ChangedLine {
            side,
            line_number,
            hunk,
            old,
            new,
        }
    }

    fn range(start: usize, end: usize) -> LineRange {
        LineRange::new(start as u64, end as u64).unwrap()
    }

    // changedLines

    #[test]
    fn lists_removed_and_added_lines_with_where_they_are_on_both_sides() {
        assert_eq!(
            changed_lines(&diff()),
            [
                changed(Side::Deletions, 3, 0, 3, 3),
                changed(Side::Additions, 3, 0, 4, 3),
                changed(Side::Additions, 4, 0, 4, 4),
                changed(Side::Deletions, 12, 1, 12, 13),
            ]
        );
    }

    #[test]
    fn numbers_the_lines_of_a_new_file_from_1() {
        let added = parse(&[
            "diff --git a/f.txt b/f.txt",
            "new file mode 100644",
            "--- /dev/null",
            "+++ b/f.txt",
            "@@ -0,0 +1,2 @@",
            "+a",
            "+b",
        ]);
        let lines: Vec<_> = changed_lines(&added)
            .iter()
            .map(|line| (line.side, line.line_number))
            .collect();
        assert_eq!(lines, [(Side::Additions, 1), (Side::Additions, 2)]);
    }

    // selectedLines

    #[test]
    fn picks_the_changed_lines_in_the_rows_selected_in_either_direction() {
        let diff = diff();
        let lines = changed_lines(&diff);
        let row_of = rows(&diff, false);
        // From the unchanged 2 to the added 3b: 3, three and 3b.
        let down = SelectedRange::new(2, Side::Additions, 4);
        let expected = LineSelection {
            deletions: vec![range(3, 3)],
            additions: vec![range(3, 4)],
        };
        assert_eq!(
            to_selection(&selected_lines(&lines, &down, &row_of)),
            expected
        );
        let up = SelectedRange {
            start: 4,
            side: Side::Additions,
            end: 3,
            end_side: Side::Deletions,
        };
        assert_eq!(
            to_selection(&selected_lines(&lines, &up, &row_of)),
            expected
        );
        // Only unchanged lines.
        let unchanged = SelectedRange::new(1, Side::Additions, 2);
        assert_eq!(selected_lines(&lines, &unchanged, &row_of), []);
    }

    #[test]
    fn picks_both_sides_of_the_rows_selected_in_split_view() {
        let diff = diff();
        let lines = changed_lines(&diff);
        // The row with 3 removed and three added, selected on its added side.
        let selected = SelectedRange::new(3, Side::Additions, 3);
        assert_eq!(
            to_selection(&selected_lines(&lines, &selected, &rows(&diff, true))),
            LineSelection {
                deletions: vec![range(3, 3)],
                additions: vec![range(3, 3)],
            }
        );
        // In unified view, only the added line.
        assert_eq!(
            to_selection(&selected_lines(&lines, &selected, &rows(&diff, false))),
            LineSelection {
                deletions: vec![],
                additions: vec![range(3, 3)],
            }
        );
    }

    #[test]
    fn picks_lines_across_hunks() {
        let diff = diff();
        let lines = changed_lines(&diff);
        let selected = SelectedRange {
            start: 4,
            side: Side::Additions,
            end: 12,
            end_side: Side::Deletions,
        };
        assert_eq!(
            to_selection(&selected_lines(&lines, &selected, &rows(&diff, false))),
            LineSelection {
                deletions: vec![range(12, 12)],
                additions: vec![range(4, 4)],
            }
        );
    }

    // changedRows

    #[test]
    fn lists_each_row_with_changes_once_in_order() {
        let diff = diff();
        let lines = changed_lines(&diff);
        let sides = |split| {
            changed_rows(&lines, &rows(&diff, split))
                .iter()
                .map(|row| (row.line.side, row.line.line_number))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            sides(false),
            [
                (Side::Deletions, 3),
                (Side::Additions, 3),
                (Side::Additions, 4),
                (Side::Deletions, 12),
            ]
        );
        // Side by side, 3 and three are on one row.
        assert_eq!(
            sides(true),
            [
                (Side::Deletions, 3),
                (Side::Additions, 4),
                (Side::Deletions, 12)
            ]
        );
    }

    // toSelection

    #[test]
    fn joins_lines_next_to_each_other_into_ranges() {
        assert_eq!(
            to_selection(&changed_lines(&diff())),
            LineSelection {
                deletions: vec![range(3, 3), range(12, 12)],
                additions: vec![range(3, 4)],
            }
        );
    }

    // rangeOf

    #[test]
    fn selects_the_rows_from_one_line_to_another() {
        let lines = changed_lines(&diff());
        assert_eq!(
            range_of(&lines[0], &lines[2]),
            SelectedRange {
                start: 3,
                side: Side::Deletions,
                end: 4,
                end_side: Side::Additions,
            }
        );
    }

    // hunkButtons

    #[test]
    fn puts_each_hunks_button_below_the_unchanged_line_before_its_first_change() {
        assert_eq!(
            hunk_buttons(&diff()),
            [
                HunkButton {
                    side: Side::Additions,
                    line_number: 2,
                    hunk: 0
                },
                HunkButton {
                    side: Side::Additions,
                    line_number: 12,
                    hunk: 1
                },
            ]
        );
    }

    #[test]
    fn puts_it_above_the_file_for_a_hunk_that_starts_with_a_change() {
        let mut lines = HEADER.to_vec();
        lines.extend(["@@ -1,2 +1,2 @@", "-1", "+one", " 2"]);
        assert_eq!(
            hunk_buttons(&parse(&lines)),
            [HunkButton {
                side: Side::Additions,
                line_number: 0,
                hunk: 0
            }]
        );
        let deleted = parse(&[
            "diff --git a/f.txt b/f.txt",
            "deleted file mode 100644",
            "--- a/f.txt",
            "+++ /dev/null",
            "@@ -1,2 +0,0 @@",
            "-a",
            "-b",
        ]);
        assert_eq!(
            hunk_buttons(&deleted),
            [HunkButton {
                side: Side::Deletions,
                line_number: 0,
                hunk: 0
            }]
        );
    }

    // ChangedRows

    #[test]
    fn looks_the_changed_lines_up_by_row_as_the_lists_have_them() {
        let diff = diff();
        let lines = changed_lines(&diff);
        for split in [false, true] {
            let row_of = rows(&diff, split);
            let by_row = ChangedRows::new(&lines, &row_of);
            assert_eq!(
                by_row.rows(),
                changed_rows(&lines, &row_of),
                "split: {split}"
            );
            for line in &lines {
                let row = row_of(line.line_number, line.side).unwrap();
                assert_eq!(
                    by_row.line_at(row, line.line_number, line.side),
                    Some(*line)
                );
                // The same lines as a selection of the rows from the first to this one.
                let first = SelectedRange::new(3, Side::Deletions, 3);
                let selected = SelectedRange {
                    end: line.line_number,
                    end_side: line.side,
                    ..first
                };
                let top = row_of(3, Side::Deletions).unwrap();
                assert_eq!(
                    by_row.in_rows(top.min(row), top.max(row)),
                    selected_lines(&lines, &selected, &row_of),
                    "split: {split}, to {line:?}"
                );
            }
            assert_eq!(by_row.line_at(0, 1, Side::Additions), None);
            assert_eq!(by_row.hunk_lines(0), &lines[..3]);
            assert_eq!(by_row.hunk_lines(1), &lines[3..]);
            let hunk: Vec<_> = by_row.hunk_rows(1).iter().map(|row| row.line).collect();
            assert_eq!(hunk, [lines[3]]);
            assert!(by_row.hunk_rows(2).is_empty());
        }
    }
}
