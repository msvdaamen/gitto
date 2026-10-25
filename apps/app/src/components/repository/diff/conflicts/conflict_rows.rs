//! The rows a conflicted file's conflicts are drawn in (see `ConflictViewer`): each conflict's
//! buttons and markers, its sides' lines, the unchanged lines around it, and separators where the
//! lines between are hidden, which can be shown. What @pierre/diffs laid out for the TS viewer from
//! the file's diff (`UnresolvedFile`), with the same rules as the diff's (see `viewer_rows`): three
//! lines of context, gaps of a line shown rather than hidden, and the same buttons to show more.

use super::super::viewer::diff_metadata::Side;
use super::super::viewer::patch_files::Expanded;
use super::super::viewer::viewer_rows::{EXPANSION_LINES, SeparatorButtons};
use super::conflict_diff::{CONTEXT_LINES, ConflictState, MarkerKind, Segment};

/// Hidden gaps of up to this many lines are shown instead (`collapsedContextThreshold`).
const COLLAPSED_CONTEXT_THRESHOLD: usize = 1;

/// What a line is in the conflicted file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineRole {
    /// Outside the conflicts, around one, or what one was resolved with.
    Context,
    /// Outside the conflicts, shown from a gap when asked.
    Expanded,
    /// Our side of a conflict.
    Ours,
    /// The base's, in a conflict with diff3 markers.
    Base,
    /// Their side of a conflict.
    Theirs,
}

/// One row of the conflicts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictRow {
    /// A line: its side and index in it (see `ConflictState::ours` and `theirs`), and its number.
    Line {
        role: LineRole,
        side: Side,
        index: usize,
        number: usize,
        /// The conflict it's in.
        conflict: Option<usize>,
    },
    /// The buttons that resolve the conflict, above it.
    Actions { conflict: usize },
    /// One of the conflict's markers.
    Marker { kind: MarkerKind, conflict: usize },
    /// Unchanged lines hidden in the gap `gap` (the index of the conflict after it, or the number
    /// of conflicts after the last one): how many, of the `range` the gap can show.
    Separator {
        gap: usize,
        hidden: usize,
        range: usize,
        buttons: SeparatorButtons,
    },
}

/// Lays `state` out in rows, with the lines shown in each gap that `expanded` says.
pub fn conflict_rows(state: &ConflictState, expanded: &Expanded) -> Vec<ConflictRow> {
    let mut rows = Vec::new();
    let segments = &state.segments;
    let anchor = |segment: Option<&Segment>| match segment {
        Some(Segment::Conflict(index) | Segment::Resolved { index, .. }) => Some(*index),
        _ => None,
    };
    for (ix, segment) in segments.iter().enumerate() {
        match segment {
            Segment::Unchanged {
                lines, theirs_at, ..
            } => {
                let before = ix.checked_sub(1).and_then(|ix| anchor(segments.get(ix)));
                let after = anchor(segments.get(ix + 1));
                let gap = after.unwrap_or(state.conflicts.len());
                let line = |offset: usize, role: LineRole| ConflictRow::Line {
                    role,
                    side: Side::Additions,
                    index: theirs_at + offset,
                    number: theirs_at + offset + 1,
                    conflict: None,
                };
                let count = lines.len();
                if before.is_some() && after.is_some() && count <= CONTEXT_LINES * 2 + 1 {
                    rows.extend((0..count).map(|offset| line(offset, LineRole::Context)));
                    continue;
                }
                let fixed_start = if before.is_some() {
                    CONTEXT_LINES.min(count)
                } else {
                    0
                };
                let fixed_end = if after.is_some() {
                    CONTEXT_LINES.min(count - fixed_start)
                } else {
                    0
                };
                let range = count - fixed_start - fixed_end;
                rows.extend((0..fixed_start).map(|offset| line(offset, LineRole::Context)));
                let expansion = expanded.get(&gap).copied().unwrap_or_default();
                let (from_start, from_end) = if range <= COLLAPSED_CONTEXT_THRESHOLD {
                    (range, 0)
                } else {
                    let from_start = expansion.from_start.min(range);
                    let from_end = expansion.from_end.min(range);
                    if from_start + from_end >= range {
                        (range, 0)
                    } else {
                        (from_start, from_end)
                    }
                };
                rows.extend(
                    (fixed_start..fixed_start + from_start)
                        .map(|offset| line(offset, LineRole::Expanded)),
                );
                let hidden = range - from_start - from_end;
                if hidden > 0 {
                    rows.push(ConflictRow::Separator {
                        gap,
                        hidden,
                        range,
                        buttons: separator_buttons(range, before.is_none(), after.is_none()),
                    });
                }
                rows.extend(
                    (fixed_start + range - from_end..fixed_start + range)
                        .map(|offset| line(offset, LineRole::Expanded)),
                );
                rows.extend(
                    (count - fixed_end..count).map(|offset| line(offset, LineRole::Context)),
                );
            }
            Segment::Resolved {
                lines, theirs_at, ..
            } => {
                rows.extend((0..lines.len()).map(|offset| ConflictRow::Line {
                    role: LineRole::Context,
                    side: Side::Additions,
                    index: theirs_at + offset,
                    number: theirs_at + offset + 1,
                    conflict: None,
                }));
            }
            Segment::Conflict(index) => {
                let Some(region) = state.conflict(*index) else {
                    continue;
                };
                let conflict = *index;
                let marker = |kind| ConflictRow::Marker { kind, conflict };
                let lines = |range: std::ops::Range<usize>, side: Side, role: LineRole| {
                    range.map(move |index| ConflictRow::Line {
                        role,
                        side,
                        index,
                        number: index + 1,
                        conflict: Some(conflict),
                    })
                };
                rows.push(ConflictRow::Actions { conflict });
                rows.push(marker(MarkerKind::Start));
                rows.extend(lines(region.ours.clone(), Side::Deletions, LineRole::Ours));
                if let Some((_, base)) = &region.base {
                    rows.push(marker(MarkerKind::Base));
                    rows.extend(lines(base.clone(), Side::Additions, LineRole::Base));
                }
                rows.push(marker(MarkerKind::Separator));
                rows.extend(lines(
                    region.theirs.clone(),
                    Side::Additions,
                    LineRole::Theirs,
                ));
                rows.push(marker(MarkerKind::End));
            }
        }
    }
    rows
}

/// The ways a gap's hidden lines can be shown, as the diff's separators have them: a hundred at a
/// time from either end, or all of a long one; a short one at once. The first gap only grows up,
/// towards the conflict after it, and the last only down.
fn separator_buttons(range: usize, first: bool, last: bool) -> SeparatorButtons {
    if range > EXPANSION_LINES {
        SeparatorButtons {
            from_start: !first,
            from_end: !last,
            both: false,
            all: true,
        }
    } else if !first && !last {
        SeparatorButtons {
            both: true,
            ..SeparatorButtons::default()
        }
    } else if first {
        SeparatorButtons {
            from_end: true,
            ..SeparatorButtons::default()
        }
    } else {
        SeparatorButtons {
            from_start: true,
            ..SeparatorButtons::default()
        }
    }
}

/// The row of the conflict `index`'s buttons, if it's laid out.
pub fn actions_row(rows: &[ConflictRow], index: usize) -> Option<usize> {
    rows.iter()
        .position(|row| *row == ConflictRow::Actions { conflict: index })
}

#[cfg(test)]
mod tests {
    use super::super::super::viewer::viewer_rows::{ExpandDirection, expand};
    use super::super::conflict_diff::{Resolution, parse_conflicts, resolve};
    use super::*;

    fn numbered(lines: usize, conflicts: &[usize]) -> String {
        let mut text = String::new();
        for line in 1..=lines {
            text.push_str(&format!("line {line}\n"));
            if conflicts.contains(&line) {
                text.push_str(&format!(
                    "<<<<<<< HEAD\nours {line}\n=======\ntheirs {line}\n>>>>>>> side\n"
                ));
            }
        }
        text
    }

    /// The numbers of the unchanged lines shown, as their text has them (`line 12`).
    fn numbers(state: &ConflictState, rows: &[ConflictRow]) -> Vec<usize> {
        rows.iter()
            .filter_map(|row| match row {
                ConflictRow::Line {
                    role: LineRole::Context | LineRole::Expanded,
                    index,
                    ..
                } => state
                    .theirs()
                    .line(*index)
                    .trim()
                    .strip_prefix("line ")?
                    .parse()
                    .ok(),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn shows_a_short_gap_between_conflicts_whole() {
        let state = parse_conflicts("f", &numbered(20, &[5, 12]), "k")
            .unwrap()
            .unwrap();
        let rows = conflict_rows(&state, &Expanded::new());
        // 3..5, 6..12 (seven lines: shown), 13..15, and the rest hidden.
        assert_eq!(
            numbers(&state, &rows),
            [3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
        );
        let separators: Vec<_> = rows
            .iter()
            .filter(|row| matches!(row, ConflictRow::Separator { .. }))
            .collect();
        assert_eq!(separators.len(), 2);
    }

    #[test]
    fn shows_more_of_a_gap_when_asked() {
        let state = parse_conflicts("f", &numbered(300, &[150]), "k")
            .unwrap()
            .unwrap();
        let mut expanded = Expanded::new();
        let rows = conflict_rows(&state, &expanded);
        let Some(ConflictRow::Separator {
            gap,
            hidden,
            range,
            buttons,
        }) = rows.first().copied()
        else {
            panic!("a separator first");
        };
        assert_eq!((gap, hidden, range), (0, 147, 147));
        // The first gap grows towards the conflict, or all at once.
        assert!(buttons.from_end && !buttons.from_start && buttons.all);

        expand(&mut expanded, gap, ExpandDirection::FromEnd, range);
        let rows = conflict_rows(&state, &expanded);
        assert_eq!(numbers(&state, &rows)[0], 48);
        expand(&mut expanded, gap, ExpandDirection::All, range);
        assert_eq!(numbers(&state, &conflict_rows(&state, &expanded))[0], 1);
    }

    #[test]
    fn keeps_a_resolved_conflicts_lines_on_show_where_it_was() {
        let state = parse_conflicts("f", &numbered(300, &[100, 200]), "k")
            .unwrap()
            .unwrap();
        let resolved = resolve(&state, 0, Resolution::Both).unwrap();
        let rows = conflict_rows(&resolved, &Expanded::new());
        // Lines 98..100, both sides' lines, and 101..103, around where the conflict was.
        let shown: Vec<&str> = rows
            .iter()
            .filter_map(|row| match row {
                ConflictRow::Line {
                    side: Side::Additions,
                    index,
                    conflict: None,
                    ..
                } => Some(resolved.theirs().line(*index)),
                _ => None,
            })
            .take(8)
            .collect();
        assert_eq!(
            shown,
            [
                "line 98\n",
                "line 99\n",
                "line 100\n",
                "ours 100\n",
                "theirs 100\n",
                "line 101\n",
                "line 102\n",
                "line 103\n"
            ]
        );
        assert_eq!(actions_row(&rows, 0), None);
        assert!(actions_row(&rows, 1).is_some());
    }
}
