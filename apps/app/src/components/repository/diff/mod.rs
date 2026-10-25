//! A file's changes, and its conflicts (`components/repository/diff/`).
//!
//! The file's view (`file_diff_view`) shows its changes with the [`viewer`], and a conflicted
//! file's conflicts with the [`conflicts`]' own. Staging lines picks them with the viewer's
//! `line_picker` and stages them with `line_staging`; a whole file is staged with `file_staging`.
//! An unstaged file, or a conflicted one, is edited in place (`file_editing`), saved a moment after
//! each edit (`autosave`).

pub mod autosave;
pub mod conflicts;
pub mod file_diff_view;
pub mod file_editing;
pub mod file_staging;
pub mod line_staging;
pub mod shown_file;
pub mod unsaved_changes_dialog;
pub mod viewer;

#[cfg(test)]
mod test_views;
