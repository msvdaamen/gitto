//! The rows a diff is drawn in, one column or side by side: its hunks' lines, the unchanged ones
//! shown around them, the separators where lines are hidden, and the hunks' buttons. What
//! @pierre/diffs laid out for the TS viewer (`iterateOverDiff`, `pushSeparator`), with the same
//! rules: which unchanged lines a gap shows (see `patch_files::Expansion`), what its separator
//! says, and which ways it can be expanded.

use std::collections::HashMap;

use super::super::line_staging::HunkButton;
use super::diff_metadata::{ChangeType, FileDiff, HunkContent, Side, end_boundary, start_boundary};
use super::patch_files::{Expanded, Expansion};
use gitto_app_data::state::diff_style::DiffStyle;

/// How many unchanged lines a separator's buttons show at a time (`expansionLineCount`).
pub const EXPANSION_LINES: usize = 100;

/// Gaps of up to this many lines are shown, rather than hidden behind a separator
/// (`collapsedContextThreshold`).
const COLLAPSED_CONTEXT_THRESHOLD: usize = 1;

/// A line of one side, by where it is in the diff's lines and its number in its file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineAt {
    pub index: usize,
    pub number: usize,
}

/// What a row's lines are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// Unchanged, in a hunk.
    Context,
    /// Unchanged, shown around a hunk from the whole file.
    Expanded,
    /// Removed, or added.
    Change,
}

/// The ways a separator's hidden lines can be shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SeparatorButtons {
    /// The lines at the top of the gap, below the hunk before (`up`).
    pub from_start: bool,
    /// The lines at the bottom of the gap, above the hunk after (`down`).
    pub from_end: bool,
    /// One button for both, for a gap of no more than [`EXPANSION_LINES`] (`both`).
    pub both: bool,
    /// All of a longer gap at once (`Expand all`).
    pub all: bool,
}

impl SeparatorButtons {
    pub fn any(self) -> bool {
        self.from_start || self.from_end || self.both || self.all
    }
}

/// Which way to show a gap's hidden lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpandDirection {
    FromStart,
    FromEnd,
    Both,
    All,
}

/// One row of the diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// A line, or side by side a removed one and an added one: an unchanged line has both.
    Line {
        kind: LineKind,
        deletion: Option<LineAt>,
        addition: Option<LineAt>,
        /// The hunk it's in, or the gap it's shown from.
        hunk: usize,
    },
    /// Unchanged lines hidden before the hunk `index` (the number of hunks: after the last);
    /// how many, if that's known.
    Separator {
        index: usize,
        hidden: Option<usize>,
        buttons: SeparatorButtons,
    },
    /// A thin line between hunks, where hidden lines can't be shown.
    Simple,
    /// The button that stages the hunk `hunk`, above its first change.
    HunkButton { hunk: usize },
    /// The line above has no line end: on the removed side, the added one, or both.
    NoNewline { deletion: bool, addition: bool },
}

/// How the hunks are separated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Separators {
    /// Saying how many lines are hidden, with buttons to show them (`line-info`).
    LineInfo,
    /// A thin line, without buttons, once the whole file couldn't be loaded (`simple`).
    Simple,
}

/// A diff's rows, and where its lines are among them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Layout {
    pub rows: Vec<Row>,
    deletion_rows: HashMap<usize, usize>,
    addition_rows: HashMap<usize, usize>,
}

impl Layout {
    /// The row the line `number` of `side` is on, if it's shown.
    pub fn row_of(&self, number: usize, side: Side) -> Option<usize> {
        match side {
            Side::Deletions => self.deletion_rows.get(&number).copied(),
            Side::Additions => self.addition_rows.get(&number).copied(),
        }
    }
}

/// What a diff is laid out with.
#[derive(Debug, Clone, Copy)]
pub struct LayoutOptions<'a> {
    pub style: DiffStyle,
    pub expanded: &'a Expanded,
    pub separators: Separators,
    /// Whether the whole file can be read to show the lines a partial diff hides.
    pub can_load_files: bool,
    pub hunk_buttons: &'a [HunkButton],
}

/// The lines a gap shows, and how many it hides (`getExpandedRegion`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Region {
    from_start: usize,
    from_end: usize,
    range: usize,
    collapsed: usize,
}

fn region(is_partial: bool, range: usize, expansion: Option<&Expansion>) -> Region {
    if range == 0 || is_partial {
        return Region {
            from_start: 0,
            from_end: 0,
            range,
            collapsed: range,
        };
    }
    if range <= COLLAPSED_CONTEXT_THRESHOLD {
        return Region {
            from_start: range,
            from_end: 0,
            range,
            collapsed: 0,
        };
    }
    let from_start = expansion.map_or(0, |region| region.from_start).min(range);
    let from_end = expansion.map_or(0, |region| region.from_end).min(range);
    let all = from_start + from_end >= range;
    Region {
        from_start: if all { range } else { from_start },
        from_end: if all { 0 } else { from_end },
        range,
        collapsed: range.saturating_sub(from_start + from_end),
    }
}

/// The unchanged lines after the last hunk of a whole file (`getTrailingContextRangeSize`): none
/// if the sides disagree, which a hydrated diff checks they don't (see `has_matching_ends`).
fn trailing_range(diff: &FileDiff) -> usize {
    let Some(last) = diff.hunks.last() else {
        return 0;
    };
    if diff.is_partial || diff.addition_lines.is_empty() || diff.deletion_lines.is_empty() {
        return 0;
    }
    let additions = diff
        .addition_lines
        .len()
        .saturating_sub(end_boundary(last.addition_start, last.addition_count));
    let deletions = diff
        .deletion_lines
        .len()
        .saturating_sub(end_boundary(last.deletion_start, last.deletion_count));
    if additions != deletions {
        return 0;
    }
    additions
}

/// Whether a partial diff's hidden lines can be read from the whole files: a change's
/// (`canHydrateCollapsedContext`).
fn can_hydrate(diff: &FileDiff, can_load_files: bool) -> bool {
    diff.is_partial
        && can_load_files
        && matches!(diff.kind, ChangeType::Change | ChangeType::RenameChanged)
}

/// Lays `diff` out in rows (see the module's doc).
pub fn layout(diff: &FileDiff, options: LayoutOptions<'_>) -> Layout {
    let mut builder = Builder {
        layout: Layout::default(),
        split: options.style == DiffStyle::Split,
        buttons: options.hunk_buttons,
    };
    let can_hydrate = can_hydrate(diff, options.can_load_files);
    let expandable = !diff.is_partial || can_hydrate;
    let separators = options.separators;
    let last_hunk = diff.hunks.len().saturating_sub(1);
    let trailing = trailing_range(diff);

    if builder.buttons.iter().any(|button| button.line_number == 0) {
        let hunk = builder.buttons[0].hunk;
        builder.layout.rows.push(Row::HunkButton { hunk });
    }

    for (index, hunk) in diff.hunks.iter().enumerate() {
        let deletion_boundary = start_boundary(hunk.deletion_start, hunk.deletion_count);
        let addition_boundary = start_boundary(hunk.addition_start, hunk.addition_count);
        let deletion_start = if !diff.is_partial && hunk.deletion_count == 0 {
            deletion_boundary
        } else {
            hunk.deletion_line_index
        };
        let addition_start = if !diff.is_partial && hunk.addition_count == 0 {
            addition_boundary
        } else {
            hunk.addition_line_index
        };
        let leading = region(
            diff.is_partial,
            hunk.collapsed_before,
            options.expanded.get(&index),
        );
        let expanded_line = |offset: usize| {
            (
                LineAt {
                    index: deletion_start + offset - leading.range,
                    number: deletion_boundary + 1 + offset - leading.range,
                },
                LineAt {
                    index: addition_start + offset - leading.range,
                    number: addition_boundary + 1 + offset - leading.range,
                },
            )
        };
        for offset in 0..leading.from_start {
            let (deletion, addition) = expanded_line(offset);
            builder.line(LineKind::Expanded, Some(deletion), Some(addition), index);
        }
        if leading.collapsed > 0 {
            builder.separator(
                separators,
                index,
                Some(leading.collapsed),
                leading.range,
                expandable,
                index == 0,
                false,
            );
        }
        for offset in leading.range - leading.from_end..leading.range {
            let (deletion, addition) = expanded_line(offset);
            builder.line(LineKind::Expanded, Some(deletion), Some(addition), index);
        }

        let mut deletion_ix = deletion_start;
        let mut addition_ix = addition_start;
        let mut deletion_number = deletion_boundary + 1;
        let mut addition_number = addition_boundary + 1;
        let contents = hunk.content.len();
        for (content_ix, content) in hunk.content.iter().enumerate() {
            let last_content = content_ix + 1 == contents;
            match *content {
                HunkContent::Context { lines, .. } => {
                    for offset in 0..lines {
                        builder.line(
                            LineKind::Context,
                            Some(LineAt {
                                index: deletion_ix + offset,
                                number: deletion_number + offset,
                            }),
                            Some(LineAt {
                                index: addition_ix + offset,
                                number: addition_number + offset,
                            }),
                            index,
                        );
                    }
                    if last_content
                        && (hunk.no_eof_newline_deletions || hunk.no_eof_newline_additions)
                    {
                        builder.layout.rows.push(Row::NoNewline {
                            deletion: hunk.no_eof_newline_deletions,
                            addition: hunk.no_eof_newline_additions,
                        });
                    }
                    deletion_ix += lines;
                    addition_ix += lines;
                    deletion_number += lines;
                    addition_number += lines;
                }
                HunkContent::Change {
                    deletions,
                    additions,
                    ..
                } => {
                    let deletion_at = |offset: usize| LineAt {
                        index: deletion_ix + offset,
                        number: deletion_number + offset,
                    };
                    let addition_at = |offset: usize| LineAt {
                        index: addition_ix + offset,
                        number: addition_number + offset,
                    };
                    let no_deletion_newline = last_content && hunk.no_eof_newline_deletions;
                    let no_addition_newline = last_content && hunk.no_eof_newline_additions;
                    if builder.split {
                        for offset in 0..deletions.max(additions) {
                            builder.line(
                                LineKind::Change,
                                (offset < deletions).then(|| deletion_at(offset)),
                                (offset < additions).then(|| addition_at(offset)),
                                index,
                            );
                        }
                        if no_deletion_newline || no_addition_newline {
                            builder.layout.rows.push(Row::NoNewline {
                                deletion: no_deletion_newline,
                                addition: no_addition_newline,
                            });
                        }
                    } else {
                        for offset in 0..deletions {
                            builder.line(LineKind::Change, Some(deletion_at(offset)), None, index);
                        }
                        if no_deletion_newline && deletions > 0 {
                            builder.layout.rows.push(Row::NoNewline {
                                deletion: true,
                                addition: false,
                            });
                        }
                        for offset in 0..additions {
                            builder.line(LineKind::Change, None, Some(addition_at(offset)), index);
                        }
                        if no_addition_newline && additions > 0 {
                            builder.layout.rows.push(Row::NoNewline {
                                deletion: false,
                                addition: true,
                            });
                        }
                    }
                    deletion_ix += deletions;
                    addition_ix += additions;
                    deletion_number += deletions;
                    addition_number += additions;
                }
            }
        }

        if index != last_hunk || separators == Separators::Simple {
            continue;
        }
        // After the last hunk: the rest of a whole file, or, of a partial one, that there may be
        // more.
        if trailing > 0 {
            let expansion = options.expanded.get(&diff.hunks.len());
            let from_start = if trailing <= COLLAPSED_CONTEXT_THRESHOLD {
                trailing
            } else {
                expansion
                    .map_or(0, |region| region.from_start)
                    .min(trailing)
            };
            for offset in 0..from_start {
                builder.line(
                    LineKind::Expanded,
                    Some(LineAt {
                        index: deletion_ix + offset,
                        number: deletion_number + offset,
                    }),
                    Some(LineAt {
                        index: addition_ix + offset,
                        number: addition_number + offset,
                    }),
                    diff.hunks.len(),
                );
            }
            if trailing > from_start {
                builder.separator(
                    separators,
                    diff.hunks.len(),
                    Some(trailing - from_start),
                    trailing,
                    expandable,
                    false,
                    true,
                );
            }
        } else if can_hydrate {
            builder.separator(
                separators,
                diff.hunks.len(),
                None,
                0,
                expandable,
                false,
                true,
            );
        }
    }
    builder.layout
}

struct Builder<'a> {
    layout: Layout,
    split: bool,
    buttons: &'a [HunkButton],
}

impl Builder<'_> {
    fn line(
        &mut self,
        kind: LineKind,
        deletion: Option<LineAt>,
        addition: Option<LineAt>,
        hunk: usize,
    ) {
        let row = self.layout.rows.len();
        if let Some(line) = deletion {
            self.layout.deletion_rows.insert(line.number, row);
        }
        if let Some(line) = addition {
            self.layout.addition_rows.insert(line.number, row);
        }
        self.layout.rows.push(Row::Line {
            kind,
            deletion,
            addition,
            hunk,
        });
        // A hunk's button goes below the unchanged line before its first change.
        if kind != LineKind::Change
            && let Some(line) = addition
            && let Some(button) = self.buttons.iter().find(|button| {
                button.side == Side::Additions
                    && button.line_number == line.number
                    && button.hunk == hunk
            })
        {
            self.layout.rows.push(Row::HunkButton { hunk: button.hunk });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn separator(
        &mut self,
        separators: Separators,
        index: usize,
        hidden: Option<usize>,
        range: usize,
        expandable: bool,
        first: bool,
        last: bool,
    ) {
        match separators {
            Separators::Simple => {
                if index > 0 && !last {
                    self.layout.rows.push(Row::Simple);
                }
            }
            Separators::LineInfo => {
                let buttons = if !expandable {
                    SeparatorButtons::default()
                } else if range > EXPANSION_LINES || hidden.is_none() {
                    // A gap of unknown size (after the last hunk) shows a hundred at a time too.
                    SeparatorButtons {
                        from_start: !first,
                        from_end: !last,
                        both: false,
                        all: hidden.is_some(),
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
                };
                self.layout.rows.push(Row::Separator {
                    index,
                    hidden,
                    buttons,
                });
            }
        }
    }
}

/// `expanded`, with the gap `index` showing [`EXPANSION_LINES`] more lines in `direction`
/// (`expandHunk`); all of it, for [`ExpandDirection::All`].
pub fn expand(expanded: &mut Expanded, index: usize, direction: ExpandDirection, range: usize) {
    let region = expanded.entry(index).or_default();
    match direction {
        ExpandDirection::FromStart => region.from_start += EXPANSION_LINES,
        ExpandDirection::FromEnd => region.from_end += EXPANSION_LINES,
        ExpandDirection::Both => {
            region.from_start += EXPANSION_LINES;
            region.from_end += EXPANSION_LINES;
        }
        ExpandDirection::All => region.from_start = region.from_start.max(range),
    }
}

/// How many unchanged lines the gap `index` of the whole-file `diff` has: before that hunk, or
/// after the last one.
pub fn gap_size(diff: &FileDiff, index: usize) -> usize {
    match diff.hunks.get(index) {
        Some(hunk) => hunk.collapsed_before,
        None => trailing_range(diff),
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::line_staging::hunk_buttons;
    use super::super::diff_metadata::{LoadedSide, hydrate, parse_patch_files};
    use super::*;

    fn numbered(count: usize, changed: usize) -> String {
        (1..=count)
            .map(|line| {
                if line == changed {
                    "changed\n".to_owned()
                } else {
                    format!("line {line}\n")
                }
            })
            .collect()
    }

    /// Line 50 of 200 changed, as git would give it, and the same filled in with the whole files.
    fn diffs() -> (FileDiff, FileDiff) {
        let partial = parse_patch_files(
            "diff --git a/f.txt b/f.txt\nindex 1111111..2222222 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -47,7 +47,7 @@\n line 47\n line 48\n line 49\n-line 50\n+changed\n line 51\n line 52\n line 53\n",
            Some("test"),
        )
        .remove(0);
        let before = numbered(200, 0);
        let after = numbered(200, 50);
        let whole = hydrate(
            &partial,
            Some(LoadedSide {
                contents: &before,
                cache_key: None,
            }),
            Some(LoadedSide {
                contents: &after,
                cache_key: None,
            }),
        )
        .unwrap();
        (partial, whole)
    }

    fn options<'a>(
        style: DiffStyle,
        expanded: &'a Expanded,
        buttons: &'a [HunkButton],
    ) -> LayoutOptions<'a> {
        LayoutOptions {
            style,
            expanded,
            separators: Separators::LineInfo,
            can_load_files: true,
            hunk_buttons: buttons,
        }
    }

    #[test]
    fn says_how_many_lines_a_patch_hides_and_that_there_may_be_more_after_it() {
        let (partial, _) = diffs();
        let expanded = Expanded::new();
        let layout = layout(&partial, options(DiffStyle::Unified, &expanded, &[]));
        assert_eq!(
            layout.rows.first(),
            Some(&Row::Separator {
                index: 0,
                hidden: Some(46),
                buttons: SeparatorButtons {
                    from_end: true,
                    ..SeparatorButtons::default()
                },
            })
        );
        assert_eq!(
            layout.rows.last(),
            Some(&Row::Separator {
                index: 1,
                hidden: None,
                buttons: SeparatorButtons {
                    from_start: true,
                    ..SeparatorButtons::default()
                },
            })
        );
        // 3 + 1 + 1 + 3 lines, in one column.
        assert_eq!(layout.rows.len(), 2 + 8);
        assert_eq!(layout.row_of(50, Side::Deletions), Some(4));
        assert_eq!(layout.row_of(50, Side::Additions), Some(5));
    }

    #[test]
    fn pairs_a_removed_line_with_the_added_one_side_by_side() {
        let (partial, _) = diffs();
        let expanded = Expanded::new();
        let layout = layout(&partial, options(DiffStyle::Split, &expanded, &[]));
        assert_eq!(layout.rows.len(), 2 + 7);
        assert_eq!(
            layout.row_of(50, Side::Deletions),
            layout.row_of(50, Side::Additions)
        );
    }

    #[test]
    fn shows_the_lines_asked_for_around_the_changes_of_a_whole_file() {
        let (_, whole) = diffs();
        let mut expanded = Expanded::new();
        expand(
            &mut expanded,
            0,
            ExpandDirection::FromEnd,
            gap_size(&whole, 0),
        );
        let rows = layout(&whole, options(DiffStyle::Unified, &expanded, &[]));
        // Only 46 lines above: all of them, with no separator left.
        assert_eq!(rows.row_of(1, Side::Additions), Some(0));
        // 147 below, in a chunk of more than a hundred: up, and all.
        assert_eq!(
            rows.rows.last(),
            Some(&Row::Separator {
                index: 1,
                hidden: Some(147),
                buttons: SeparatorButtons {
                    from_start: true,
                    all: true,
                    ..SeparatorButtons::default()
                },
            })
        );
        expand(&mut expanded, 1, ExpandDirection::All, gap_size(&whole, 1));
        let rows = layout(&whole, options(DiffStyle::Unified, &expanded, &[]));
        assert_eq!(rows.row_of(200, Side::Additions), Some(rows.rows.len() - 1));
    }

    #[test]
    fn puts_each_hunks_button_below_the_line_before_its_change() {
        let (partial, _) = diffs();
        let expanded = Expanded::new();
        let buttons = hunk_buttons(&partial);
        let layout = layout(&partial, options(DiffStyle::Unified, &expanded, &buttons));
        assert_eq!(layout.rows[4], Row::HunkButton { hunk: 0 });
        assert_eq!(layout.row_of(50, Side::Deletions), Some(5));
    }
}
