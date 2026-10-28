//! The viewer of a file's changes: `patch_viewer` draws the rows gitto-ui-logic's
//! `diff::viewer_rows` lays out from the patch its `diff::diff_metadata` parses, highlighted off
//! the main thread (`highlight`, `viewer_runtime`). Lines to stage are picked with `line_picker`
//! and gitto-ui-logic's `diff::line_staging`; reading whole files to show more lines fits them
//! with its `diff::patch_files`; editing in place is `viewer_editing`. Its keys are actions
//! (`viewer_actions`).

pub mod highlight;
pub mod line_picker;
pub mod patch_viewer;
pub mod viewer_actions;
pub mod viewer_editing;
pub mod viewer_runtime;
