//! A file's changes, and its conflicts (`components/repository/diff/`).
//!
//! The file's view (`file_diff_view`) shows its changes with the viewer (`patch_viewer`), which
//! draws the rows `viewer_rows` lays out from the patch `diff_metadata` parses, highlighted off the
//! main thread (`highlight`, `viewer_runtime`). Staging lines picks them with `line_picker` and
//! `line_staging`; reading whole files to show more lines fits them with `patch_files`.
//!
//! A conflicted file's conflicts are resolved in the same view (`conflict_resolution`), one by one
//! in the conflicts' viewer (`conflict_viewer`, drawing the rows `conflict_rows` lays out from what
//! `conflict_diff` reads), or by keeping a side whole (`conflict_sides`). An unstaged file, or a
//! conflicted one, is edited in place (`file_editing`, `viewer_editing`, `conflict_whole_file`),
//! saved a moment after each edit (`autosave`).

pub mod autosave;
pub mod conflict_diff;
pub mod conflict_resolution;
pub mod conflict_rows;
pub mod conflict_sides;
pub mod conflict_viewer;
pub mod conflict_whole_file;
pub mod diff_metadata;
pub mod file_diff_view;
pub mod file_editing;
pub mod file_staging;
pub mod highlight;
pub mod line_picker;
pub mod line_staging;
pub mod markers_dialog;
pub mod patch_files;
pub mod patch_viewer;
pub mod shown_file;
pub mod unsaved_changes_dialog;
pub mod viewer_editing;
pub mod viewer_rows;
pub mod viewer_runtime;

#[cfg(test)]
mod test_views;
