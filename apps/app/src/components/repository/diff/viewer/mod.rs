//! The viewer of a file's changes: `patch_viewer` draws the rows `viewer_rows` lays out from the
//! patch `diff_metadata` parses, highlighted off the main thread (`highlight`, `viewer_runtime`).
//! Lines to stage are picked with `line_picker`; reading whole files to show more lines fits them
//! with `patch_files`; editing in place is `viewer_editing`.

pub mod diff_metadata;
pub mod highlight;
pub mod line_picker;
pub mod patch_files;
pub mod patch_viewer;
pub mod viewer_editing;
pub mod viewer_rows;
pub mod viewer_runtime;
