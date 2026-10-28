//! What Gitto's UI works out for itself, without GPUI: ported from `apps/ui/src`, for the GPUI app
//! in `apps/app` to show. Each module is a TS file, by the same path:
//!
//! - [`git`]: `git/*.ts` (outside `git/queries/`), what the UI makes of what gitto-git gives it:
//!   the history's graph ([`git::graph`]) and rows ([`git::rows`]), the labels of its branches and
//!   tags ([`git::ref_labels`]), its search ([`git::search`]), the sidebar's tree of refs
//!   ([`git::ref_tree`]), commit messages ([`git::message`]), patches ([`git::patch`]), conflicts
//!   ([`git::conflicts`]), and what to call things.
//! - [`diff`]: `components/repository/diff/*.ts`'s model of a file's changes: a patch parsed
//!   ([`diff::diff_metadata`]), the viewer's rows ([`diff::viewer_rows`]), the lines picked to stage
//!   ([`diff::line_staging`]) and whole files fitted to a patch ([`diff::patch_files`]).
//! - [`format`]: `lib/format.ts`: initials, avatar colours, relative times and counts.
//! - [`hooks`]: what of `hooks/*.ts` isn't Solid's signals: the clock relative times are told by,
//!   which sidebar folders are collapsed, and panels' widths, saved in a [`hooks::Storage`] as the
//!   Electron app saved them in `localStorage`.
//!
//! Where the TS used `@gitto/git/types`, this uses gitto-git's types.

pub mod diff;
pub mod format;
pub mod git;
pub mod hooks;
