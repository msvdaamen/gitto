//! What the features' `handler.ts` did beyond passing the input on to a command. The others are
//! elsewhere: `status.get`'s `since` is `get_status_since`, `history.log`'s defaults are `Page`'s,
//! and the watches are `GitApi`'s, which open the repository for them.

use crate::core::errors::Result;
use crate::core::repo::Repo;
use crate::features::remote::commands::{pull as pull_current, pull_branch};
use crate::input::BranchShortName;

/// Pulls a branch's upstream into it (`remote.pull`): the checked-out branch's, or, with `branch`,
/// that local branch's, by its short name. One that isn't checked out is only fast-forwarded; that
/// fails with `Error::PullBlocked`, doing nothing, if it diverged from its upstream, or is in use
/// in a worktree (see `pull_branch`).
pub async fn pull(repo: &Repo, branch: Option<&BranchShortName>) -> Result<()> {
    match branch {
        None => pull_current(repo).await,
        Some(branch) => pull_branch(repo, branch).await,
    }
}
