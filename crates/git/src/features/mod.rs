//! What Gitto does with git, one module per feature, each ported from its folder in
//! `packages/git/src/features/`.

pub mod branch;
pub mod commit;
pub mod conflicts;
pub mod diff;
pub mod history;
pub mod operation;
pub mod overview;
pub mod refs;
pub mod remote;
pub mod staging;
pub mod stash;
pub mod status;
pub mod user;
pub mod watch;
pub mod worktree;
