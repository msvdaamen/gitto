//! The keys of a file's changes, as actions: picking lines to stage, and scrolling, in the patch's
//! viewer (`PatchViewer`); scrolling in the conflicts' viewer (`ConflictViewer`), the same way;
//! resolving a conflicted file's conflicts, and going back to the history, from the file's view
//! (`FileDiffView`). Bound in [`key_bindings`], so they can be rebound, listed and dispatched.

use gpui_kit::{KeyBinding, actions};

/// The key context of the patch's viewer.
pub const PATCH_VIEWER: &str = "PatchViewer";
/// The key context of the conflicts' viewer, but while it shows the whole file, whose editor has
/// the keys then.
pub const CONFLICT_VIEWER: &str = "ConflictViewer";
/// The key context of a file's view.
pub const FILE_DIFF_VIEW: &str = "FileDiffView";
/// Said of a file's view, in its key context, while its conflicts take the keys: they're on show to
/// resolve, and the file isn't edited by hand.
pub const RESOLVING: &str = "resolving";

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
        /// Scrolls down a line or two, as the arrow keys do in a browser.
        ScrollDown,
        ScrollUp,
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

actions!(
    conflicts,
    [
        /// Keeps our side of the current conflict.
        KeepOurs,
        /// Keeps their side of the current conflict.
        KeepTheirs,
        /// Keeps both sides of the current conflict, ours first.
        KeepBoth,
        /// Steps to the next conflict.
        NextConflict,
        /// Steps to the conflict before.
        PreviousConflict,
        /// Opens the next conflicted file.
        OpenNextConflicted,
        /// Marks the file resolved, asking first if it has conflict markers left.
        MarkResolved,
    ]
);

/// The keys of a file's changes: the viewers', as browsers scroll, and the file's view's.
pub fn key_bindings() -> Vec<KeyBinding> {
    let patch = Some(PATCH_VIEWER);
    let mut bindings = vec![
        KeyBinding::new("down", SelectDown, patch),
        KeyBinding::new("up", SelectUp, patch),
        KeyBinding::new("shift-down", ExtendSelectionDown, patch),
        KeyBinding::new("shift-up", ExtendSelectionUp, patch),
        KeyBinding::new("]", SelectNextHunk, patch),
        KeyBinding::new("[", SelectPreviousHunk, patch),
        KeyBinding::new("space", StageSelection, patch),
        KeyBinding::new("escape", ClearSelection, patch),
    ];
    bindings.extend(scroll_bindings(PATCH_VIEWER));

    let conflicts = Some(CONFLICT_VIEWER);
    bindings.extend([
        KeyBinding::new("down", ScrollDown, conflicts),
        KeyBinding::new("up", ScrollUp, conflicts),
        KeyBinding::new("space", ScrollPageDown, conflicts),
    ]);
    bindings.extend(scroll_bindings(CONFLICT_VIEWER));

    // Wherever the focus is in the file's view, but in an editor, which the file's only in while
    // it's edited.
    let resolving = format!("{FILE_DIFF_VIEW} && {RESOLVING}");
    let resolving = Some(resolving.as_str());
    bindings.extend([
        KeyBinding::new("o", KeepOurs, resolving),
        KeyBinding::new("t", KeepTheirs, resolving),
        KeyBinding::new("b", KeepBoth, resolving),
        KeyBinding::new("]", NextConflict, resolving),
        KeyBinding::new("[", PreviousConflict, resolving),
        KeyBinding::new("n", OpenNextConflicted, resolving),
        KeyBinding::new("cmd-enter", MarkResolved, resolving),
        KeyBinding::new("ctrl-enter", MarkResolved, resolving),
        KeyBinding::new("escape", CloseFileDiff, Some(FILE_DIFF_VIEW)),
    ]);
    bindings
}

/// The keys both viewers scroll with, as a browser's scroll container does, in the key context
/// `context`.
fn scroll_bindings(context: &str) -> Vec<KeyBinding> {
    let context = Some(context);
    vec![
        KeyBinding::new("left", ScrollLeft, context),
        KeyBinding::new("right", ScrollRight, context),
        KeyBinding::new("pagedown", ScrollPageDown, context),
        KeyBinding::new("pageup", ScrollPageUp, context),
        KeyBinding::new("shift-space", ScrollPageUp, context),
        KeyBinding::new("home", ScrollToTop, context),
        KeyBinding::new("end", ScrollToBottom, context),
    ]
}
