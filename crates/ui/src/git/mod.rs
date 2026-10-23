//! What the UI makes of what gitto-git gives it: ported from `apps/ui/src/git/*.ts`, one module
//! per file (`ref-labels.ts` → [`ref_labels`]). `git/queries/`, which fetched it, is the app's.

pub mod activity;
pub mod changes;
pub mod conflicts;
pub mod diff_source;
pub mod graph;
pub mod message;
pub mod patch;
pub mod push;
pub mod ref_labels;
pub mod ref_tree;
pub mod remote;
pub mod rows;
pub mod search;
pub mod stash;
pub mod status;
pub mod worktree;
