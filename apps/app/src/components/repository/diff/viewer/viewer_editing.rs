//! Editing the file on show in the diff view (see `file_editing`): the working tree's file, read
//! whole, in GPUI Kit's code editor, its changed lines marked, in the viewer's place. Ported from
//! `viewer-editing.ts`, which had @pierre/diffs' editor edit the diff's new side in place; this
//! edits the whole file the same way, and the diff is shown again, with the edits, once editing
//! stops.

use std::ops::Range;
use std::sync::Arc;

use gpui_kit::component::input::{
    EditorState, InputEvent, RangeDecoration, RangeDecorationCollection, RangeDecorationStyle,
};
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, EventEmitter, Subscription, Window, div};

use super::super::conflicts::conflict_whole_file::{code_editor, render_code_editor, reveal_line};
use super::super::file_editing::EditSession;
use super::diff_metadata::{ChangeType, FileDiff, HunkContent, Side};
use super::patch_files::fit_to_patch;
use super::viewer_runtime::{LoadFile, LoadedFile};
use gitto_app_kit::theme::PaletteExt as _;
use gitto_app_kit::ui::LoadingState;

/// What editing the file tells the file's view.
#[derive(Debug, Clone, PartialEq)]
pub enum PatchEditorEvent {
    /// Editing started (see `EditSession`), or stopped.
    Editing(Option<EditSession>),
    /// The file was edited to the text it has.
    Edit(String),
    /// Why editing couldn't start, e.g. as the file changed again meanwhile.
    EditFailed(String),
}

/// The lines `diff` adds, by their number in the new file (from 1), each range over them.
pub fn added_lines(diff: &FileDiff) -> Vec<Range<usize>> {
    let mut added = Vec::new();
    for hunk in &diff.hunks {
        let mut number = hunk.addition_start.max(1);
        for content in &hunk.content {
            match *content {
                HunkContent::Context { lines, .. } => number += lines,
                HunkContent::Change { additions, .. } => {
                    if additions > 0 {
                        added.push(number..number + additions);
                    }
                    number += additions;
                }
            }
        }
    }
    added
}

/// Where each of `lines` (by number, from 1) is in `text`, its line end included.
fn line_ranges(text: &str, lines: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut starts = vec![0];
    starts.extend(text.match_indices('\n').map(|(at, _)| at + 1));
    lines
        .iter()
        .filter_map(|range| {
            let start = *starts.get(range.start - 1)?;
            let end = starts.get(range.end - 1).copied().unwrap_or(text.len());
            (end > start).then_some(start..end)
        })
        .collect()
}

/// The working tree's file to edit: read whole, and checked against the patch on show (`diff`),
/// so what's edited is what was shown. Fails with why it can't be edited.
pub fn editable_text(diff: &FileDiff, read: LoadedFile) -> Result<(String, String), String> {
    let LoadedFile::WorkingTree { contents, version } = read else {
        return Err("This file can't be edited.".to_owned());
    };
    let contents = if diff.kind == ChangeType::New && !diff.is_partial {
        contents
    } else {
        fit_to_patch(diff, Side::Additions, &contents)?
    };
    Ok((contents, version))
}

/// The file on show, being edited: read whole first, then in an editor, from the first change.
pub struct PatchEditor {
    editor: Option<Entity<EditorState>>,
    _decorations: Option<RangeDecorationCollection>,
    _changes: Option<Subscription>,
}

impl EventEmitter<PatchEditorEvent> for PatchEditor {}

impl PatchEditor {
    /// Starts editing the file `diff` (the patch on show) is of, at `path`, read with `load_file`.
    pub fn new(
        diff: Arc<FileDiff>,
        path: String,
        load_file: LoadFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let reading = load_file(
            Side::Additions,
            diff.new_object_id.as_deref().unwrap_or_default(),
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let read = reading.await;
            let _ = this.update_in(cx, |this, window, cx| {
                let edited = read
                    .map_err(|error| error.to_string())
                    .and_then(|read| editable_text(&diff, read));
                match edited {
                    Ok((contents, version)) => {
                        this.open(&diff, &path, &contents, window, cx);
                        cx.emit(PatchEditorEvent::Editing(Some(EditSession { version })));
                    }
                    Err(message) => cx.emit(PatchEditorEvent::EditFailed(message)),
                }
            });
        })
        .detach();
        PatchEditor {
            editor: None,
            _decorations: None,
            _changes: None,
        }
    }

    /// Puts `contents` in the editor, its added lines marked, from the first change.
    fn open(
        &mut self,
        diff: &FileDiff,
        path: &str,
        contents: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editor = code_editor(path, contents, window, cx);
        let added = added_lines(diff);
        let tint = cx.palette().mint.opacity(0.14);
        let decorations = line_ranges(contents, &added)
            .into_iter()
            .map(|range| {
                RangeDecoration::new(range)
                    .with_style(RangeDecorationStyle::Fill)
                    .with_color(tint)
            })
            .collect();
        let collection = editor.update(cx, |editor, cx| {
            editor.set_readonly(false, cx);
            editor.create_range_decorations_collection(decorations, cx)
        });
        self._changes = Some(cx.subscribe(&editor, |_, editor, event, cx| {
            if matches!(event, InputEvent::Change) {
                let text = editor.read(cx).value().to_string();
                cx.emit(PatchEditorEvent::Edit(text));
            }
        }));
        let first = added.first().map_or(0, |range| range.start - 1);
        reveal_line(&editor, first, window, cx);
        self._decorations = Some(collection);
        self.editor = Some(editor);
        cx.notify();
    }

    /// The editor, once the file's read.
    pub fn editor(&self) -> Option<&Entity<EditorState>> {
        self.editor.as_ref()
    }

    /// The text the edits left, once there's an editor.
    pub fn text(&self, cx: &App) -> Option<String> {
        self.editor
            .as_ref()
            .map(|editor| editor.read(cx).value().to_string())
    }

    /// Whether there's a selection in the editor for Escape to collapse.
    pub fn has_selection(&self, cx: &App) -> bool {
        self.editor
            .as_ref()
            .is_some_and(|editor| !editor.read(cx).selected_range().is_empty())
    }
}

impl Render for PatchEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = *cx.palette();
        div()
            .size_full()
            .min_h_0()
            .bg(palette.bg)
            .child(match &self.editor {
                Some(editor) => render_code_editor(editor, false),
                None => LoadingState::new("editor-loading", "Loading changes…")
                    .fill()
                    .into_any_element(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::super::diff_metadata::parse_patch_files;
    use super::*;

    #[test]
    fn marks_the_lines_a_patch_adds_by_their_number_in_the_file() {
        let patch = "diff --git a/a.txt b/a.txt\nindex 1..2 100644\n--- a/a.txt\n+++ b/a.txt\n@@ -1,3 +1,4 @@\n one\n-two\n+TWO\n+2\n three\n";
        let diff = parse_patch_files(patch, None).remove(0);
        let added = added_lines(&diff);
        assert_eq!((added.len(), added[0].clone()), (1, 2..4));
        let ranges = line_ranges("one\nTWO\n2\nthree\n", &added);
        assert_eq!((ranges.len(), ranges[0].clone()), (1, 4..10));
    }

    #[test]
    fn edits_the_working_tree_file_only_if_its_the_one_on_show() {
        let patch = "diff --git a/a.txt b/a.txt\nindex 1..2 100644\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-old\n+new\n";
        let diff = parse_patch_files(patch, None).remove(0);
        let read = |contents: &str| LoadedFile::WorkingTree {
            contents: contents.into(),
            version: "v1".into(),
        };
        assert_eq!(
            editable_text(&diff, read("new\n")),
            Ok(("new\n".into(), "v1".into()))
        );
        assert!(editable_text(&diff, read("changed again\n")).is_err());
        assert_eq!(
            editable_text(&diff, LoadedFile::Blob("new\n".into())),
            Err("This file can't be edited.".into())
        );
    }
}
