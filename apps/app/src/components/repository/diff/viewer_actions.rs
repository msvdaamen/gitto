//! The keys of a file's changes, as actions: picking lines to stage, and scrolling, in the patch's
//! viewer (`PatchViewer`), and going back to the history from the file's view (`FileDiffView`).
//! Bound in [`key_bindings`], so they can be rebound, listed and dispatched.

use gpui_kit::{KeyBinding, actions};

/// The key context of the patch's viewer.
pub const PATCH_VIEWER: &str = "PatchViewer";
/// The key context of a file's view.
pub const FILE_DIFF_VIEW: &str = "FileDiffView";

actions!(
    diff_viewer,
    [
        /// Selects the next changed line down; scrolls down a line where lines can't be picked.
        SelectDown,
        /// Selects the next changed line up; scrolls up a line where lines can't be picked.
        SelectUp,
        /// Extends the selection to the next changed line down; scrolls where lines can't be
        /// picked.
        ExtendSelectionDown,
        /// Extends the selection to the next changed line up; scrolls where lines can't be picked.
        ExtendSelectionUp,
        /// Selects the next hunk's changes.
        SelectNextHunk,
        /// Selects the previous hunk's changes.
        SelectPreviousHunk,
        /// Stages the lines picked; without any, scrolls down a page, as Space does elsewhere.
        StageSelection,
        /// Clears the selection; without one, goes on to close the changes.
        ClearSelection,
        ScrollLeft,
        ScrollRight,
        ScrollPageDown,
        ScrollPageUp,
        ScrollToTop,
        ScrollToBottom,
    ]
);

actions!(
    file_diff,
    [
        /// Goes back to the history.
        CloseFileDiff
    ]
);

/// The keys of a file's changes: the viewer's, as browsers scroll, and the file's view's.
pub fn key_bindings() -> Vec<KeyBinding> {
    let mut bindings = viewer_bindings(PATCH_VIEWER);
    bindings.push(KeyBinding::new(
        "escape",
        CloseFileDiff,
        Some(FILE_DIFF_VIEW),
    ));
    bindings
}

/// The keys of the viewer whose key context is `context`.
fn viewer_bindings(context: &str) -> Vec<KeyBinding> {
    let context = Some(context);
    vec![
        KeyBinding::new("down", SelectDown, context),
        KeyBinding::new("up", SelectUp, context),
        KeyBinding::new("shift-down", ExtendSelectionDown, context),
        KeyBinding::new("shift-up", ExtendSelectionUp, context),
        KeyBinding::new("]", SelectNextHunk, context),
        KeyBinding::new("[", SelectPreviousHunk, context),
        KeyBinding::new("space", StageSelection, context),
        KeyBinding::new("escape", ClearSelection, context),
        KeyBinding::new("left", ScrollLeft, context),
        KeyBinding::new("right", ScrollRight, context),
        KeyBinding::new("pagedown", ScrollPageDown, context),
        KeyBinding::new("pageup", ScrollPageUp, context),
        KeyBinding::new("shift-space", ScrollPageUp, context),
        KeyBinding::new("home", ScrollToTop, context),
        KeyBinding::new("end", ScrollToBottom, context),
    ]
}
