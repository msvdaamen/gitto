//! The local branches that are in use in a worktree, which git won't delete, nor Gitto pull
//! without checking them out.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use futures_lite::StreamExt;

pub use crate::core::errors::InUse;
use crate::core::errors::Result;
use crate::core::repo::GitCommand;

/// What the file at `path` says, trimmed; `""` if it isn't there.
async fn read(path: PathBuf) -> String {
    async_fs::read_to_string(path)
        .await
        .map(|text| text.trim().to_owned())
        .unwrap_or_default()
}

/// The git directories of the linked worktrees of the git directory `common_dir`; none if it has
/// none.
async fn linked_worktrees(common_dir: &Path) -> Vec<PathBuf> {
    let Ok(mut entries) = async_fs::read_dir(common_dir.join("worktrees")).await else {
        return Vec::new();
    };
    let mut linked = Vec::new();
    while let Some(Ok(entry)) = entries.next().await {
        linked.push(common_dir.join("worktrees").join(entry.file_name()));
    }
    linked
}

/// The local branches git won't delete, nor Gitto pull without checking them out, by their full
/// names, each with why and what to do about it: checked out in a worktree, or being rebased or
/// bisected in one, which detaches its HEAD meanwhile. Read from each worktree's git directory,
/// where git keeps those, as it does.
pub async fn branches_in_use(run: &GitCommand) -> Result<HashMap<String, InUse>> {
    let (heads, common) = futures::try_join!(
        run.run([
            "for-each-ref",
            "--format=%(refname)%00%(worktreepath)",
            "refs/heads/"
        ]),
        run.run(["rev-parse", "--path-format=absolute", "--git-common-dir"]),
    )?;
    let mut in_use = HashMap::new();
    for line in heads.split('\n') {
        let mut fields = line.split('\0');
        let (local, worktree) = (fields.next().unwrap_or(""), fields.next().unwrap_or(""));
        if !local.is_empty() && !worktree.is_empty() {
            in_use.insert(
                local.to_owned(),
                InUse::CheckedOut {
                    worktree: PathBuf::from(worktree),
                },
            );
        }
    }

    let common_dir = PathBuf::from(common.trim());
    let mut dirs = vec![common_dir.clone()];
    dirs.extend(linked_worktrees(&common_dir).await);
    let states = futures::future::join_all(dirs.iter().map(|dir| async move {
        futures::join!(
            read(dir.join("rebase-merge").join("head-name")),
            read(dir.join("rebase-apply").join("head-name")),
            read(dir.join("BISECT_START")),
        )
    }))
    .await;
    for (rebase_merge, rebase_apply, bisect) in states {
        for rebased in [rebase_merge, rebase_apply] {
            if rebased.starts_with("refs/heads/") {
                in_use.entry(rebased).or_insert(InUse::Rebasing);
            }
        }
        // The branch's short name; a commit's SHA when bisecting started detached.
        if !bisect.is_empty() {
            in_use
                .entry(format!("refs/heads/{bisect}"))
                .or_insert(InUse::Bisecting);
        }
    }
    Ok(in_use)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::test_support::{Fixture, git};

    #[test]
    fn are_checked_out_rebased_or_bisected_in_a_worktree() {
        let fx = Fixture::new();
        let path = fx.create_repo("in-use");
        git(&path, ["commit", "-q", "--allow-empty", "-m", "first"]);
        git(&path, ["branch", "rebased"]);
        git(&path, ["branch", "bisected"]);
        git(&path, ["branch", "free"]);
        let linked = fx.root().join("in-use-linked");
        git(
            &path,
            [
                "worktree",
                "add",
                "-q",
                "-b",
                "linked",
                linked.to_str().unwrap(),
            ],
        );
        // As a rebase stopped in the linked worktree, and bisecting started here, leave them.
        let linked_dir = path.join(".git/worktrees/in-use-linked");
        fs::create_dir(linked_dir.join("rebase-merge")).unwrap();
        fs::write(
            linked_dir.join("rebase-merge/head-name"),
            "refs/heads/rebased\n",
        )
        .unwrap();
        fs::write(path.join(".git/BISECT_START"), "bisected\n").unwrap();

        smol::block_on(async {
            let repo = fx.repos.open("in-use").await.unwrap();
            let in_use = branches_in_use(&repo.git()).await.unwrap();
            assert_eq!(
                in_use["refs/heads/main"],
                InUse::CheckedOut {
                    worktree: path.clone()
                }
            );
            assert_eq!(
                in_use["refs/heads/linked"].state(),
                format!("is checked out in {}", linked.display())
            );
            assert_eq!(in_use["refs/heads/rebased"], InUse::Rebasing);
            assert_eq!(in_use["refs/heads/bisected"].state(), "is being bisected");
            assert_eq!(in_use["refs/heads/bisected"].fix(), "Stop bisecting first.");
            assert!(!in_use.contains_key("refs/heads/free"));
        });
    }
}
