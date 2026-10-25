//! What adding a worktree takes (see `super::changes`). Ported from `git/queries/worktree.ts`.

/// What a worktree is added with: the folder, the branch (by its full ref name), and a new
/// branch's name, if one is made from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddWorktreeInput {
    pub path: String,
    pub branch: String,
    pub new_branch: Option<String>,
}
