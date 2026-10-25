//! A conflicted file shown whole, in place of its conflicts (see `ConflictViewer`): one without
//! conflicts left, or whose markers can't be read as conflicts, and one being edited by hand,
//! markers and all. Ported from `conflict-whole-file.ts`, which had @pierre/diffs' virtualized file
//! and editor; this is GPUI Kit's code editor, which draws only the lines on screen too, and
//! highlights them itself.

use gpui_kit::component::input::{Editor, EditorState, InputEvent, Position};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Context, Entity, Subscription, Window, point, rems};

use super::super::viewer::patch_viewer::{FONT_REMS, ROW_REMS};
use super::conflict_viewer::{ConflictViewer, ConflictViewerEvent};

/// The language a file's path names, for the editor's highlighting: its extension, or its whole
/// name (`Makefile`), as the diff's highlighting finds it.
pub fn editor_language(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((_, extension)) if !extension.is_empty() => extension.to_owned(),
        _ => name.to_owned(),
    }
}

/// A file's text in GPUI Kit's code editor, as the diff shows code: in its font and size, with
/// line numbers, without wrapping or folding. Read-only unless it's being edited.
pub fn code_editor<V>(
    path: &str,
    contents: &str,
    window: &mut Window,
    cx: &mut Context<V>,
) -> Entity<EditorState> {
    let language = editor_language(path);
    let contents = contents.to_owned();
    cx.new(|cx| {
        let mut state = EditorState::new(window, cx)
            .language(language)
            .line_number(true)
            .folding(false)
            .indent_guides(false)
            .soft_wrap(false)
            .default_value(contents);
        state.set_readonly(true, cx);
        state
    })
}

/// Draws `state` as the diff draws code.
pub fn render_code_editor(state: &Entity<EditorState>, readonly: bool) -> AnyElement {
    Editor::new(state)
        .readonly(readonly)
        .bordered(false)
        .appearance(false)
        .text_size(rems(FONT_REMS))
        .line_height(rems(ROW_REMS))
        .size_full()
        .into_any_element()
}

/// Puts the cursor on `line` (from 0), and the line a third of the way down the view, focusing the
/// editor. A new editor isn't laid out yet: it's scrolled once it is, on the next frame.
pub fn reveal_line(state: &Entity<EditorState>, line: usize, window: &mut Window, cx: &mut App) {
    let laid_out = state.update(cx, |state, cx| {
        state.set_cursor_position(
            Position {
                line: line as u32,
                character: 0,
            },
            window,
            cx,
        );
        let Some(height) = state.line_height() else {
            return false;
        };
        let view = state.input_bounds().size.height;
        let top = (height * line as f32 - view / 3.).max(gpui_kit::px(0.));
        let offset = state.scroll_offset();
        state.set_scroll_offset(point(offset.x, -top), cx);
        true
    });
    if !laid_out {
        let state = state.downgrade();
        window.on_next_frame(move |window, cx| {
            if let Some(state) = state.upgrade()
                && state.read(cx).line_height().is_some()
            {
                reveal_line(&state, line, window, cx);
            }
        });
    }
}

/// The whole file, in place of the conflicts: shown, edited, or neither (`createWholeFile`).
#[derive(Default)]
pub struct WholeFile {
    editor: Option<Entity<EditorState>>,
    editing: bool,
    _changes: Option<Subscription>,
}

impl WholeFile {
    /// Whether it's on show.
    pub fn is_shown(&self) -> bool {
        self.editor.is_some()
    }

    pub fn is_editing(&self) -> bool {
        self.editing
    }

    pub fn editor(&self) -> Option<&Entity<EditorState>> {
        self.editor.as_ref()
    }

    /// Whether there's a selection in the editor for Escape to collapse.
    pub fn has_selection(&self, cx: &App) -> bool {
        self.editing
            && self
                .editor
                .as_ref()
                .is_some_and(|editor| !editor.read(cx).selected_range().is_empty())
    }

    /// Shows `contents` whole, read-only.
    pub fn show(
        &mut self,
        path: &str,
        contents: &str,
        window: &mut Window,
        cx: &mut Context<ConflictViewer>,
    ) {
        self.stop_editing(cx);
        match &self.editor {
            Some(editor) if editor.read(cx).value().as_ref() == contents => {}
            _ => {
                let editor = code_editor(path, contents, window, cx);
                // What's typed, while it's edited, is told.
                self._changes = Some(cx.subscribe(&editor, |this, editor, event, cx| {
                    if matches!(event, InputEvent::Change) && this.whole.is_editing() {
                        let text = editor.read(cx).value().to_string();
                        cx.emit(ConflictViewerEvent::Edit(text));
                    }
                }));
                self.editor = Some(editor);
            }
        }
    }

    /// Shows the conflicts again, in its place.
    pub fn hide(&mut self) {
        self.editing = false;
        self.editor = None;
        self._changes = None;
    }

    /// Shows `contents` whole and edits it, at line `line` (from 0) if given, a third of the way
    /// down the view, where the conflict it's of was among the conflicts.
    pub fn edit(
        &mut self,
        path: &str,
        contents: &str,
        line: Option<usize>,
        window: &mut Window,
        cx: &mut Context<ConflictViewer>,
    ) {
        self.show(path, contents, window, cx);
        let Some(editor) = self.editor.clone() else {
            return;
        };
        self.editing = true;
        editor.update(cx, |editor, cx| editor.set_readonly(false, cx));
        match line {
            Some(line) => reveal_line(&editor, line, window, cx),
            None => editor.update(cx, |editor, cx| editor.focus(window, cx)),
        }
    }

    /// Stops editing, if it was; returns the text the edits left, `None` if it wasn't being
    /// edited. They stay on show, read-only, until the viewer shows what it keeps.
    pub fn stop_editing<V>(&mut self, cx: &mut Context<V>) -> Option<String> {
        if !self.editing {
            return None;
        }
        self.editing = false;
        let editor = self.editor.as_ref()?;
        let text = editor.read(cx).value().to_string();
        editor.update(cx, |editor, cx| editor.set_readonly(true, cx));
        Some(text)
    }

    pub fn render(&self) -> Option<AnyElement> {
        self.editor
            .as_ref()
            .map(|editor| render_code_editor(editor, !self.editing))
    }
}
