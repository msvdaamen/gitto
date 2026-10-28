//! What adding a worktree takes (see `super::changes`). Ported from `git/queries/worktree.ts`.

use gitto_git::Result;
use gitto_git::input::{BranchName, BranchRef, Folder};

/// What a worktree is added with: the folder, the branch (by its full ref name), and a new
/// branch's name, if one is made from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddWorktreeInput {
    pub path: Folder,
    pub branch: BranchRef,
    pub new_branch: Option<BranchName>,
}

impl AddWorktreeInput {
    /// The worktree at `path`, of `branch`, or of a new branch `new_branch` made from it, if
    /// they're valid.
    pub fn parse(
        path: impl Into<String>,
        branch: impl Into<String>,
        new_branch: Option<String>,
    ) -> Result<Self> {
        Ok(AddWorktreeInput {
            path: Folder::parse(path)?,
            branch: BranchRef::parse(branch)?,
            new_branch: new_branch.map(BranchName::parse).transpose()?,
        })
    }
}
