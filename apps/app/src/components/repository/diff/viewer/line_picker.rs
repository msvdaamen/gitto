//! Picking lines of the patch on show to stage or unstage (see `PatchViewer`'s staging):
//! selecting them with the mouse or the keyboard, by line or a hunk at a time, and what the
//! buttons that stage them stage. Which lines a selection picks, and the rows to move it between,
//! is `line_staging`'s. Ported from `line-picker.tsx`; the viewer draws the selection and the
//! buttons, and scrolls to what this says to reveal.

use std::rc::Rc;

use gitto_git::features::staging::schema::LineSelection;
use gpui_kit::{App, Task};

use super::super::line_staging::{
    ChangedLine, ChangedRow, SelectedRange, changed_lines, changed_rows, range_of, selected_lines,
};
use super::diff_metadata::{FileDiff, Side};
use super::viewer_rows::Layout;
use gitto_app_data::data::DataError;
use gitto_app_data::data::staging::StageAction;

/// Stages `lines` of `patch`, the patch on show they were picked from; resolves once they're
/// staged, and fails if they couldn't be.
pub type StageLines = Rc<dyn Fn(String, LineSelection, &mut App) -> Task<Result<(), DataError>>>;

/// Staging or unstaging lines of the patch on show, which can then be picked (`LineStaging`).
#[derive(Clone)]
pub struct LineStaging {
    pub action: StageAction,
    /// Lines are being staged: no others are until the patch that leaves is on show.
    pub busy: bool,
    pub stage: StageLines,
}

impl PartialEq for LineStaging {
    fn eq(&self, other: &Self) -> bool {
        self.action == other.action
            && self.busy == other.busy
            && Rc::ptr_eq(&self.stage, &other.stage)
    }
}

/// Where the keyboard staged lines from, by the line numbers of the side the patch leaves as it
/// is (the working tree's, staging; HEAD's, unstaging): the change found there in the next patch
/// on show is selected (see [`LinePicker::shown`]), or its hunk, if they were picked a hunk at a
/// time; if they couldn't be staged, the same lines are, in the patch reloaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StagedAt {
    pub side: Side,
    pub position: usize,
    pub hunks: bool,
}

/// The rows in view, to move from when nothing's selected.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    /// The first row whose top is in view, as a fraction: the scroll offset in rows.
    pub top: f32,
    /// How many rows the view is tall.
    pub rows: f32,
}

/// Lines to stage, and from where the keyboard staged them, if it did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageRequest {
    pub lines: Vec<ChangedLine>,
    pub at: Option<StagedAt>,
}

/// What a key did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyOutcome {
    /// Not the picker's: it goes on, e.g. to scroll, or to close the changes.
    Ignored,
    /// Handled; the line given is to be scrolled into view.
    Handled { reveal: Option<ChangedLine> },
    /// Handled: the hunk from the first line to the last is to be scrolled into view.
    RevealHunk {
        first: ChangedLine,
        last: ChangedLine,
    },
    /// Space: stage the lines picked.
    Stage,
}

/// The keys the picker has (`aria-keyshortcuts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerKey {
    Up { extend: bool },
    Down { extend: bool },
    NextHunk,
    PreviousHunk,
    Space,
    Escape,
}

/// Picks lines to stage in the patch on show (see the module's doc): by selecting them, with the
/// mouse or with the arrow keys (Shift to extend), or a hunk at a time, with its button or `[` and
/// `]`. The button in the gutter, by the changed line the pointer is on or the last one selected,
/// or Space stages them; Esc clears the selection. Once the keyboard staged some, the selection
/// moves to the change that's in their place.
#[derive(Debug, Default)]
pub struct LinePicker {
    /// The changed lines of the diff on show.
    lines: Vec<ChangedLine>,
    selection: Option<SelectedRange>,
    /// The changed lines selected.
    picked: Vec<ChangedLine>,
    /// Whether the selection is of whole hunks, gone through with `[` and `]`, not by line.
    by_hunk: bool,
    staged_at: Option<StagedAt>,
    /// The changed line the pointer is on, if any, which the gutter's button stages without a
    /// selection. Forgotten once another patch is on show, where it may be another line, until
    /// the pointer moves.
    hovered: Option<ChangedLine>,
}

impl LinePicker {
    pub fn selection(&self) -> Option<&SelectedRange> {
        self.selection.as_ref()
    }

    pub fn picked(&self) -> &[ChangedLine] {
        &self.picked
    }

    pub fn hovered(&self) -> Option<&ChangedLine> {
        self.hovered.as_ref()
    }

    pub fn staged_at(&self) -> Option<StagedAt> {
        self.staged_at
    }

    /// The changed line `number` of `side`, if it's one.
    pub fn changed_line(&self, number: usize, side: Side) -> Option<ChangedLine> {
        self.lines
            .iter()
            .find(|line| line.side == side && line.line_number == number)
            .copied()
    }

    /// Selects `range`, or nothing.
    pub fn select(&mut self, range: Option<SelectedRange>, layout: &Layout) {
        self.by_hunk = false;
        self.picked = match &range {
            Some(range) => selected_lines(&self.lines, range, &row_of(layout)),
            None => Vec::new(),
        };
        self.selection = range;
    }

    /// The pointer is on the changed line `line`, or on none.
    pub fn hover(&mut self, line: Option<ChangedLine>) -> bool {
        let changed = self.hovered != line;
        self.hovered = line;
        changed
    }

    /// What the gutter's button stages: the lines selected, or else the line the pointer is on.
    pub fn to_stage(&self) -> Vec<ChangedLine> {
        if self.selection.is_some() {
            self.picked.clone()
        } else {
            self.hovered.into_iter().collect()
        }
    }

    /// The changed lines of the hunk `hunk`.
    pub fn hunk_lines(&self, hunk: usize) -> Vec<ChangedLine> {
        self.lines
            .iter()
            .filter(|line| line.hunk == hunk)
            .copied()
            .collect()
    }

    /// Selects the changes of `line`'s hunk, to go through them a hunk at a time.
    fn select_hunk(&mut self, line: &ChangedLine, layout: &Layout) -> Vec<ChangedRow> {
        let hunk: Vec<ChangedRow> = changed_rows(&self.lines, &row_of(layout))
            .into_iter()
            .filter(|row| row.line.hunk == line.hunk)
            .collect();
        if let (Some(first), Some(last)) = (hunk.first(), hunk.last()) {
            self.select(Some(range_of(&first.line, &last.line)), layout);
            self.by_hunk = true;
        }
        hunk
    }

    /// `changed`, to stage with `action`, unless there are none. Remembers where the keyboard
    /// staged from. Whether lines can be staged now (not while another stage is under way, nor
    /// from a patch other than the one picked from) is the viewer's to check first.
    pub fn stage_request(
        &mut self,
        changed: Vec<ChangedLine>,
        by_keyboard: bool,
        action: StageAction,
    ) -> Option<StageRequest> {
        if changed.is_empty() {
            return None;
        }
        let side = match action {
            StageAction::Stage => Side::Additions,
            StageAction::Unstage => Side::Deletions,
        };
        let at = by_keyboard.then(|| StagedAt {
            side,
            position: changed
                .iter()
                .map(|line| line.position(side))
                .min()
                .unwrap_or(0),
            hunks: self.by_hunk,
        });
        self.staged_at = at;
        Some(StageRequest { lines: changed, at })
    }

    /// The lines asked for weren't staged, e.g. as the file changed since: the patch reloaded is
    /// selected in the same place, if another is on its way, and the selection stays where it is
    /// otherwise.
    pub fn stage_failed(&mut self, at: Option<StagedAt>, awaiting_patch: bool) {
        if self.staged_at == at && !awaiting_patch {
            self.staged_at = None;
        }
    }

    /// `diff`, the patch's, is on show: in place of the last file's, or of the same file's with
    /// `same_file`. The selection is cleared, the lines picked being of the last patch: their
    /// numbers may be others' in this one. Selects the change in place of the lines the keyboard
    /// staged, if it did, and says which to reveal.
    pub fn shown(
        &mut self,
        diff: &FileDiff,
        same_file: bool,
        pickable: bool,
        layout: &Layout,
    ) -> Option<ChangedLine> {
        self.lines = changed_lines(diff);
        self.hovered = None;
        self.select(None, layout);
        self.reselect(same_file, pickable, layout)
    }

    /// Selects the change in place of the lines the keyboard staged, in the patch they left, so
    /// the next is a key away: the first one after where they were, on the side that didn't
    /// change, or its hunk if they were picked a hunk at a time. In another file (the next one,
    /// once the last lines were staged), its first change, or hunk.
    fn reselect(
        &mut self,
        same_file: bool,
        pickable: bool,
        layout: &Layout,
    ) -> Option<ChangedLine> {
        let at = self.staged_at.take()?;
        if !pickable {
            return None;
        }
        let rows = changed_rows(&self.lines, &row_of(layout));
        let target = if same_file {
            rows.iter()
                .find(|row| row.line.position(at.side) >= at.position)
                .or(rows.last())
        } else {
            rows.first()
        }?;
        let line = target.line;
        if at.hunks {
            self.select_hunk(&line, layout);
        } else {
            self.select(Some(range_of(&line, &line)), layout);
        }
        Some(line)
    }

    /// The first changed row from the top of the view, or the last one above its bottom.
    fn in_view(&self, down: bool, layout: &Layout, viewport: Viewport) -> Option<ChangedRow> {
        let rows = changed_rows(&self.lines, &row_of(layout));
        if down {
            rows.iter()
                .find(|row| row.row as f32 >= viewport.top)
                .or(rows.last())
                .copied()
        } else {
            rows.iter()
                .rev()
                .find(|row| (row.row as f32) < viewport.top + viewport.rows)
                .or(rows.first())
                .copied()
        }
    }

    /// The rows the selection spans, top and bottom, and the one it was extended to.
    fn selected_rows(range: &SelectedRange, layout: &Layout) -> (usize, usize, usize) {
        let start = layout.row_of(range.start, range.side).unwrap_or(0);
        let end = layout.row_of(range.end, range.end_side).unwrap_or(0);
        (start.min(end), start.max(end), end)
    }

    /// Selects the next changed row down or up from the selection, or extends the selection to
    /// it; without a selection, the first one in view.
    fn move_selection(
        &mut self,
        down: bool,
        extend: bool,
        layout: &Layout,
        viewport: Viewport,
    ) -> Option<ChangedLine> {
        let rows = changed_rows(&self.lines, &row_of(layout));
        let target = match self.selection {
            None => self.in_view(down, layout, viewport),
            Some(selected) => {
                let (top, bottom, end) = Self::selected_rows(&selected, layout);
                let from = if extend {
                    end
                } else if down {
                    bottom
                } else {
                    top
                };
                if down {
                    rows.iter().find(|row| row.row > from).copied()
                } else {
                    rows.iter().rev().find(|row| row.row < from).copied()
                }
            }
        }?;
        let range = match (extend, self.selection) {
            (true, Some(selected)) => SelectedRange {
                end: target.line.line_number,
                end_side: target.line.side,
                ..selected
            },
            _ => range_of(&target.line, &target.line),
        };
        self.select(Some(range), layout);
        Some(target.line)
    }

    /// Selects the next hunk's changes down or up from the selection, or the first one in view.
    fn move_to_hunk(
        &mut self,
        down: bool,
        layout: &Layout,
        viewport: Viewport,
    ) -> Option<(ChangedLine, ChangedLine)> {
        let rows = changed_rows(&self.lines, &row_of(layout));
        let target = match self.selection {
            None => self.in_view(down, layout, viewport).map(|row| row.line),
            Some(selected) => {
                let (top, bottom, _) = Self::selected_rows(&selected, layout);
                // The first row of each hunk, down from the selection, or up from its top.
                let firsts: Vec<&ChangedRow> = rows
                    .iter()
                    .enumerate()
                    .filter(|(ix, row)| *ix == 0 || rows[ix - 1].line.hunk != row.line.hunk)
                    .map(|(_, row)| row)
                    .collect();
                if down {
                    firsts
                        .iter()
                        .find(|row| row.row > bottom)
                        .map(|row| row.line)
                } else {
                    firsts
                        .iter()
                        .rev()
                        .find(|row| row.row < top)
                        .map(|row| row.line)
                }
            }
        }?;
        let hunk = self.select_hunk(&target, layout);
        Some((hunk.first()?.line, hunk.last()?.line))
    }

    /// Handles `key`, if lines can be picked from the patch on show (`pickable`, `live`).
    pub fn key(
        &mut self,
        key: PickerKey,
        pickable: bool,
        live: bool,
        layout: &Layout,
        viewport: Viewport,
    ) -> KeyOutcome {
        if !pickable || !live {
            return KeyOutcome::Ignored;
        }
        match key {
            PickerKey::Down { extend } | PickerKey::Up { extend } => {
                let down = matches!(key, PickerKey::Down { .. });
                let reveal = self.move_selection(down, extend, layout, viewport);
                KeyOutcome::Handled { reveal }
            }
            PickerKey::NextHunk | PickerKey::PreviousHunk => {
                match self.move_to_hunk(key == PickerKey::NextHunk, layout, viewport) {
                    Some((first, last)) => KeyOutcome::RevealHunk { first, last },
                    None => KeyOutcome::Handled { reveal: None },
                }
            }
            // Without lines selected, Space scrolls, as it does elsewhere.
            PickerKey::Space if self.picked.is_empty() => KeyOutcome::Ignored,
            PickerKey::Space => KeyOutcome::Stage,
            // Without a selection, Esc goes on to close the changes.
            PickerKey::Escape if self.selection.is_none() => KeyOutcome::Ignored,
            PickerKey::Escape => {
                self.select(None, layout);
                KeyOutcome::Handled { reveal: None }
            }
        }
    }
}

/// The row a line is on in `layout`.
fn row_of(layout: &Layout) -> impl Fn(usize, Side) -> Option<usize> + '_ {
    move |number, side| layout.row_of(number, side)
}

#[cfg(test)]
mod tests {
    use super::super::super::line_staging::hunk_buttons;
    use super::super::diff_metadata::parse_patch_files;
    use super::super::patch_files::Expanded;
    use super::super::viewer_rows::{LayoutOptions, Separators, layout};
    use super::*;
    use gitto_app_data::state::diff_style::DiffStyle;

    fn diff() -> FileDiff {
        parse_patch_files(
            "diff --git a/f.txt b/f.txt\n--- a/f.txt\n+++ b/f.txt\n@@ -1,5 +1,6 @@\n 1\n 2\n-3\n+three\n+3b\n 4\n 5\n@@ -10,5 +11,4 @@\n 10\n 11\n-12\n 13\n 14\n",
            Some("test"),
        )
        .remove(0)
    }

    fn laid_out(diff: &FileDiff) -> Layout {
        let buttons = hunk_buttons(diff);
        layout(
            diff,
            LayoutOptions {
                style: DiffStyle::Unified,
                expanded: &Expanded::new(),
                separators: Separators::LineInfo,
                can_load_files: false,
                hunk_buttons: &buttons,
            },
        )
    }

    const VIEW: Viewport = Viewport { top: 0., rows: 40. };

    fn selected(picker: &LinePicker) -> Vec<(Side, usize)> {
        picker
            .picked()
            .iter()
            .map(|line| (line.side, line.line_number))
            .collect()
    }

    #[test]
    fn moves_through_the_changed_lines_with_the_arrows_and_extends_with_shift() {
        let diff = diff();
        let layout = laid_out(&diff);
        let mut picker = LinePicker::default();
        picker.shown(&diff, false, true, &layout);

        picker.key(PickerKey::Down { extend: false }, true, true, &layout, VIEW);
        assert_eq!(selected(&picker), [(Side::Deletions, 3)]);
        picker.key(PickerKey::Down { extend: true }, true, true, &layout, VIEW);
        assert_eq!(
            selected(&picker),
            [(Side::Deletions, 3), (Side::Additions, 3)]
        );
        picker.key(PickerKey::Down { extend: false }, true, true, &layout, VIEW);
        assert_eq!(selected(&picker), [(Side::Additions, 4)]);
        picker.key(PickerKey::Up { extend: false }, true, true, &layout, VIEW);
        assert_eq!(selected(&picker), [(Side::Additions, 3)]);
    }

    #[test]
    fn goes_through_the_hunks_with_brackets() {
        let diff = diff();
        let layout = laid_out(&diff);
        let mut picker = LinePicker::default();
        picker.shown(&diff, false, true, &layout);

        picker.key(PickerKey::NextHunk, true, true, &layout, VIEW);
        assert_eq!(
            selected(&picker),
            [
                (Side::Deletions, 3),
                (Side::Additions, 3),
                (Side::Additions, 4)
            ]
        );
        picker.key(PickerKey::NextHunk, true, true, &layout, VIEW);
        assert_eq!(selected(&picker), [(Side::Deletions, 12)]);
        picker.key(PickerKey::PreviousHunk, true, true, &layout, VIEW);
        assert_eq!(selected(&picker).len(), 3);
    }

    #[test]
    fn leaves_space_and_esc_alone_without_a_selection() {
        let diff = diff();
        let layout = laid_out(&diff);
        let mut picker = LinePicker::default();
        picker.shown(&diff, false, true, &layout);
        assert_eq!(
            picker.key(PickerKey::Space, true, true, &layout, VIEW),
            KeyOutcome::Ignored
        );
        assert_eq!(
            picker.key(PickerKey::Escape, true, true, &layout, VIEW),
            KeyOutcome::Ignored
        );
        picker.key(PickerKey::Down { extend: false }, true, true, &layout, VIEW);
        assert_eq!(
            picker.key(PickerKey::Space, true, true, &layout, VIEW),
            KeyOutcome::Stage
        );
        picker.key(PickerKey::Escape, true, true, &layout, VIEW);
        assert!(picker.selection().is_none());
        // Nothing's picked from a patch that can't be staged from.
        assert_eq!(
            picker.key(
                PickerKey::Down { extend: false },
                false,
                true,
                &layout,
                VIEW
            ),
            KeyOutcome::Ignored
        );
    }

    #[test]
    fn selects_the_change_in_place_of_the_lines_the_keyboard_staged() {
        let diff = diff();
        let layout = laid_out(&diff);
        let mut picker = LinePicker::default();
        picker.shown(&diff, false, true, &layout);
        picker.key(PickerKey::Down { extend: false }, true, true, &layout, VIEW);
        let request = picker
            .stage_request(picker.picked().to_vec(), true, StageAction::Stage)
            .unwrap();
        assert_eq!(
            request.at,
            Some(StagedAt {
                side: Side::Additions,
                position: 3,
                hunks: false
            })
        );

        // The patch they leave: 3 staged, three and 3b still to stage.
        let left = parse_patch_files(
            "diff --git a/f.txt b/f.txt\n--- a/f.txt\n+++ b/f.txt\n@@ -1,4 +1,6 @@\n 1\n 2\n+three\n+3b\n 4\n 5\n",
            Some("left"),
        )
        .remove(0);
        let layout = laid_out(&left);
        let reveal = picker.shown(&left, true, true, &layout);
        assert_eq!(reveal.map(|line| line.line_number), Some(3));
        assert_eq!(selected(&picker), [(Side::Additions, 3)]);
    }
}
