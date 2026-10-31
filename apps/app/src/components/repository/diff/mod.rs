//! A file's changes, and its conflicts (`components/repository/diff/`).
//!
//! The file's view (`file_diff_view`) shows its changes with the viewer (`patch_viewer`), which
//! draws the rows gitto-ui's `diff::viewer_rows` lays out from the patch its `diff::diff_metadata`
//! parses, highlighted off the main thread (`highlight`, `viewer_runtime`). Staging lines picks
//! them with `line_picker` and gitto-ui's `diff::line_staging`; reading whole files to show more
//! lines fits them with its `diff::patch_files`. Their keys are actions (`viewer_actions`).

pub mod conflict_viewer;
pub mod file_diff_view;
pub mod file_staging;
pub mod highlight;
pub mod line_picker;
pub mod patch_viewer;
pub mod shown_file;
pub mod viewer_actions;
pub mod viewer_runtime;
