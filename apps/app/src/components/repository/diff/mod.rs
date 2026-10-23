//! A file's changes, and its conflicts (`components/repository/diff/`).
//!
//! The file's view (`file_diff_view`) shows its changes with the viewer (`patch_viewer`), which
//! draws the rows `viewer_rows` lays out from the patch `diff_metadata` parses, highlighted off the
//! main thread (`highlight`, `viewer_runtime`). Staging lines picks them with `line_picker` and
//! `line_staging`; reading whole files to show more lines fits them with `patch_files`.

pub mod conflict_viewer;
pub mod diff_metadata;
pub mod file_diff_view;
pub mod file_staging;
pub mod highlight;
pub mod line_picker;
pub mod line_staging;
pub mod patch_files;
pub mod patch_viewer;
pub mod shown_file;
pub mod viewer_rows;
pub mod viewer_runtime;
