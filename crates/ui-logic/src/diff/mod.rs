//! A file's changes as the diff viewer reads them, without GPUI: ported from what the TS viewer
//! took from @pierre/diffs, and from `components/repository/diff/*.ts`.
//!
//! - [`diff_metadata`]: a patch, parsed into its hunks and the lines of both its sides, and filled
//!   in with the whole files.
//! - [`viewer_rows`]: the rows a diff is drawn in, one column or side by side.
//! - [`line_staging`]: which changed lines a selection of rows picks to stage.
//! - [`patch_files`]: fitting whole files to a patch, to show the lines around its changes.

pub mod diff_metadata;
pub mod line_staging;
pub mod patch_files;
pub mod viewer_rows;
