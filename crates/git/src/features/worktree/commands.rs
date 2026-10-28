//! Listing, adding and removing worktrees. Ported from `commands.ts`.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use super::parse::{WORKTREE_LIST_ARGS, parse_worktree_list};
use super::schema::Worktree;
use crate::core::errors::{Error, Result, WorktreeRemovalBlocker};
use crate::core::git_dirs::real_path;
use crate::core::repo::{GitCommand, Repo};
use crate::features::branch::commands::{
    Tracking, branch_target, is_ref, no_ref_syntax_advice, not_a_branch, tracking_branch,
};
use crate::features::refs::schema::RefKind;
use crate::input::{BranchName, BranchRef, Folder};

/// The worktrees: the main one first, then the linked ones by their paths, with the one the
/// repository was opened as marked `current`.
pub async fn list_worktrees(repo: &Repo) -> Result<Vec<Worktree>> {
    Ok(listed(repo.path(), &repo.git()).await?.0)
}

/// Checks the branch `branch` (a full ref name) out in a new worktree at `folder` (an absolute
/// path, or one relative to the repository), which must be empty or not there yet. With
/// `new_branch`, a branch of that name is created from it and checked out there instead, tracking
/// it if it's a remote one. For a remote branch without one, that's the local branch tracking it,
/// or a new one that does, named after it without the remote (see `tracking_branch`). Git refuses
/// a branch that's checked out in a worktree already, the main one included, and a folder that's
/// in use, saying so.
///
/// Fails with `Error::InvalidInput` if `branch` isn't a branch that's there, as the UI's input is
/// checked.
pub async fn add_worktree(
    repo: &Repo,
    folder: &Folder,
    branch: &BranchRef,
    new_branch: Option<&BranchName>,
) -> Result<()> {
    let (folder, branch) = (folder.as_str(), branch.as_str());
    let new_branch = new_branch.map(BranchName::as_str);
    let target = branch_target(branch)?;
    let path = resolve(repo.path(), folder);
    // One write, so nothing else changes the branches halfway.
    repo.exclusive(|run| async move {
        // That ref itself, rather than git reading `branch` as a revision, like
        // `refs/heads/main~3`.
        if !is_ref(&run, branch).await? {
            return Err(not_a_branch(branch));
        }
        let add = |args: Vec<OsString>| {
            let mut all: Vec<OsString> = ["worktree", "add", "--quiet"]
                .into_iter()
                .map(OsString::from)
                .collect();
            all.extend(args);
            run.run_with(all, no_ref_syntax_advice())
        };
        let os = |arg: &str| OsString::from(arg);
        let path = path.into_os_string();
        let added = if let Some(new_branch) = new_branch {
            // Only a remote branch is tracked, so pulling the new branch doesn't pull a local
            // one's commits into it (see `create_branch`).
            let track = if target.kind == RefKind::Remote {
                "--track"
            } else {
                "--no-track"
            };
            add(vec![
                os(track),
                os("-b"),
                os(new_branch),
                os("--"),
                path,
                os(branch),
            ])
            .await
        } else if target.kind == RefKind::Local {
            add(vec![os("--"), path, os(&target.name)]).await
        } else {
            match tracking_branch(&run, branch, &target.name).await? {
                Tracking::Existing(existing) => add(vec![os("--"), path, os(&existing)]).await,
                Tracking::Local(local) => {
                    add(vec![
                        os("--track"),
                        os("-b"),
                        os(&local),
                        os("--"),
                        path,
                        os(branch),
                    ])
                    .await
                }
            }
        };
        added.map_err(Error::without_hints)?;
        Ok(())
    })
    .await
}

/// Removes the worktree at `path`, as listed: its folder is deleted, uncommitted changes and all.
/// Its branch is kept. Not the main worktree, nor the one the repository was opened as, which is
/// on show (`Error::WorktreeRemovalBlocked`); a locked one is refused by git, saying so.
pub async fn remove_worktree(repo: &Repo, path: &str) -> Result<()> {
    let root = repo.path().to_owned();
    // One write, so no other change of the worktrees can move it between finding and removing it.
    repo.exclusive(|run| async move {
        let Some(worktree) = find_worktree(&root, &run, path).await? else {
            return Err(Error::RepositoryChanged(
                "The worktree is gone: the worktrees changed before it could be removed."
                    .to_owned(),
            ));
        };
        if worktree.main {
            return Err(Error::WorktreeRemovalBlocked(WorktreeRemovalBlocker::Main));
        }
        if worktree.current {
            return Err(Error::WorktreeRemovalBlocked(
                WorktreeRemovalBlocker::Current,
            ));
        }
        run.run(["worktree", "remove", "--force", "--", &worktree.path])
            .await
            .map_err(Error::without_hints)?;
        Ok(())
    })
    .await
}

/// The worktree at `path` among those listed through `run`, by where its folder really is.
async fn find_worktree(root: &Path, run: &GitCommand, path: &str) -> Result<Option<Worktree>> {
    let (listed, wanted) = futures::join!(listed(root, run), real_path_or_itself(Path::new(path)));
    let (worktrees, paths) = listed?;
    Ok(worktrees
        .into_iter()
        .zip(paths)
        .find(|(_, real)| *real == wanted)
        .map(|(worktree, _)| worktree))
}

/// The worktrees listed through `run`: the main one first, then the linked ones by their paths,
/// with the one at `root`, where the repository was opened, marked `current`; and where each
/// one's folder really is, in the same order.
async fn listed(root: &Path, run: &GitCommand) -> Result<(Vec<Worktree>, Vec<PathBuf>)> {
    let mut worktrees = parse_worktree_list(&run.run(WORKTREE_LIST_ARGS).await?);
    if worktrees.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    // Git lists the linked ones as it finds their folders, in no set order.
    worktrees[1..].sort_by_cached_key(|worktree| locale_key(&worktree.path));
    let (here, paths) = futures::join!(
        real_path_or_itself(root),
        futures::future::join_all(
            worktrees
                .iter()
                .map(|worktree| real_path_or_itself(Path::new(&worktree.path)))
        ),
    );
    for (worktree, path) in worktrees.iter_mut().zip(&paths) {
        worktree.current = *path == here;
    }
    Ok((worktrees, paths))
}

/// What orders paths roughly as JavaScript's `localeCompare` does: by their letters regardless of
/// case first, then lowercase before uppercase. Punctuation is ordered by code point, where ICU has
/// an order of its own (`a-b`, `a.b`, `a_b` here; `a_b`, `a-b`, `a.b` in TS), which only moves a
/// worktree about in the list.
fn locale_key(path: &str) -> (String, String) {
    // Lowercase first, as ICU's tertiary strength has it.
    let mut swapped_case = String::with_capacity(path.len());
    for c in path.chars() {
        if c.is_uppercase() {
            swapped_case.extend(c.to_lowercase());
        } else {
            swapped_case.extend(c.to_uppercase());
        }
    }
    (path.to_lowercase(), swapped_case)
}

/// Where `path` really is, symlinks resolved; `path` itself if it's gone.
async fn real_path_or_itself(path: &Path) -> PathBuf {
    real_path(path).await.unwrap_or_else(|_| path.to_owned())
}

/// `folder` resolved against `base`, as Node's `path.resolve` does: an absolute `folder` as it
/// is, a relative one joined to `base`, and `.` and `..` worked out without looking at the disk.
fn resolve(base: &Path, folder: &str) -> PathBuf {
    let mut resolved = PathBuf::new();
    for component in base.join(folder).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                // Never above the root.
                if matches!(
                    resolved.components().next_back(),
                    Some(Component::Normal(_))
                ) {
                    resolved.pop();
                }
            }
            other => resolved.push(other),
        }
    }
    resolved
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::test_support::{Fixture, git, valid};

    /// A repository with `a.txt` committed on `main`, and a `feature` branch, which `repos` opens
    /// as `name`.
    fn create_committed_repo(fx: &Fixture, name: &str) -> PathBuf {
        let path = fx.create_repo(name);
        fs::write(path.join("a.txt"), "a\n").unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-qm", "first"]);
        git(&path, ["branch", "feature"]);
        path
    }

    /// The worktrees as `name [branch]`, with `*` for the current one and `!` for a locked one.
    async fn summary(fx: &Fixture, name: &str) -> Vec<String> {
        let repo = fx.repos.open(name).await.unwrap();
        list_worktrees(&repo)
            .await
            .unwrap()
            .iter()
            .map(|worktree| {
                format!(
                    "{}{}{} [{}]",
                    worktree.name,
                    if worktree.current { "*" } else { "" },
                    if worktree.locked.is_none() { "" } else { "!" },
                    worktree.branch.as_deref().unwrap_or("detached")
                )
            })
            .collect()
    }

    fn s(path: &Path) -> &str {
        path.to_str().unwrap()
    }

    fn strs(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|line| (*line).to_owned()).collect()
    }

    // Listing worktrees.

    #[test]
    fn lists_the_main_worktree_alone_as_the_current_one() {
        let fx = Fixture::new();
        let path = create_committed_repo(&fx, "alone");
        smol::block_on(async {
            let repo = fx.repos.open("alone").await.unwrap();
            assert_eq!(
                list_worktrees(&repo).await.unwrap(),
                vec![Worktree {
                    path: s(&path).to_owned(),
                    name: "alone".to_owned(),
                    head: Some(git(&path, ["rev-parse", "HEAD"])),
                    branch: Some("refs/heads/main".to_owned()),
                    main: true,
                    bare: false,
                    current: true,
                    locked: None,
                    prunable: None,
                }]
            );
        });
    }

    #[test]
    fn lists_the_linked_worktrees_after_it_on_a_branch_or_detached_and_which_is_locked() {
        let fx = Fixture::new();
        let path = create_committed_repo(&fx, "linked");
        let feature = fx.root().join("linked-feature");
        let check = fx.root().join("linked-check");
        git(&path, ["worktree", "add", "-q", s(&feature), "feature"]);
        git(&path, ["worktree", "add", "-q", "--detach", s(&check)]);
        git(&path, ["worktree", "lock", s(&check), "--reason", "busy"]);

        smol::block_on(async {
            assert_eq!(
                summary(&fx, "linked").await,
                strs(&[
                    "linked* [refs/heads/main]",
                    "linked-check! [detached]",
                    "linked-feature [refs/heads/feature]",
                ])
            );
            let repo = fx.repos.open("linked").await.unwrap();
            let check = list_worktrees(&repo).await.unwrap().remove(1);
            assert_eq!(
                check.locked.and_then(|lock| lock.reason).as_deref(),
                Some("busy")
            );
            assert_eq!(check.head, Some(git(&path, ["rev-parse", "HEAD"])));
        });
    }

    #[test]
    fn marks_the_linked_worktree_current_when_the_repository_is_opened_as_it() {
        let fx = Fixture::new();
        let path = create_committed_repo(&fx, "opened");
        let linked = fx.root().join("opened-feature");
        git(&path, ["worktree", "add", "-q", s(&linked), "feature"]);
        fx.add_path("opened-feature", &linked);

        smol::block_on(async {
            assert_eq!(
                summary(&fx, "opened-feature").await,
                strs(&[
                    "opened [refs/heads/main]",
                    "opened-feature* [refs/heads/feature]"
                ])
            );
        });
    }

    // Adding a worktree.

    #[test]
    fn checks_a_local_branch_out_in_a_new_folder_the_parents_made_as_needed() {
        let fx = Fixture::new();
        create_committed_repo(&fx, "add");
        let folder = fx.root().join("add-worktrees").join("feature");

        smol::block_on(async {
            let repo = fx.repos.open("add").await.unwrap();
            add_worktree(
                &repo,
                &valid(s(&folder)),
                &valid("refs/heads/feature"),
                None,
            )
            .await
            .unwrap();
            assert_eq!(git(&folder, ["symbolic-ref", "HEAD"]), "refs/heads/feature");
            assert!(folder.join("a.txt").exists());
            assert_eq!(
                summary(&fx, "add").await,
                strs(&["add* [refs/heads/main]", "feature [refs/heads/feature]"])
            );
        });
    }

    #[test]
    fn takes_a_folder_relative_to_the_repository() {
        let fx = Fixture::new();
        create_committed_repo(&fx, "relative");

        smol::block_on(async {
            let repo = fx.repos.open("relative").await.unwrap();
            add_worktree(
                &repo,
                &valid("../relative-feature"),
                &valid("refs/heads/feature"),
                None,
            )
            .await
            .unwrap();
        });
        assert_eq!(
            git(fx.root().join("relative-feature"), ["symbolic-ref", "HEAD"]),
            "refs/heads/feature"
        );
    }

    #[test]
    fn creates_a_new_branch_from_a_local_one_there_not_tracking_it() {
        let fx = Fixture::new();
        let path = create_committed_repo(&fx, "new-branch");
        let folder = fx.root().join("new-branch-topic");

        smol::block_on(async {
            let repo = fx.repos.open("new-branch").await.unwrap();
            add_worktree(
                &repo,
                &valid(s(&folder)),
                &valid("refs/heads/main"),
                Some(&valid("topic")),
            )
            .await
            .unwrap();
        });
        assert_eq!(git(&folder, ["symbolic-ref", "HEAD"]), "refs/heads/topic");
        assert_eq!(
            git(&path, ["rev-parse", "topic"]),
            git(&path, ["rev-parse", "main"])
        );
        assert_eq!(
            git(
                &path,
                ["for-each-ref", "--format=%(upstream)", "refs/heads/topic"]
            ),
            ""
        );
        // The branch is still checked out here.
        assert_eq!(git(&path, ["symbolic-ref", "HEAD"]), "refs/heads/main");
    }

    #[test]
    fn creates_a_branch_tracking_a_remote_one_there_or_uses_the_one_that_tracks_it() {
        let fx = Fixture::new();
        let path = create_committed_repo(&fx, "remote");
        let clone = fx.clone_repo("remote-clone", &path);
        let feature = fx.root().join("remote-clone-feature");
        smol::block_on(async {
            let repo = fx.repos.open("remote-clone").await.unwrap();

            add_worktree(
                &repo,
                &valid(s(&feature)),
                &valid("refs/remotes/origin/feature"),
                None,
            )
            .await
            .unwrap();
            assert_eq!(
                git(&feature, ["symbolic-ref", "HEAD"]),
                "refs/heads/feature"
            );
            assert_eq!(
                git(
                    &clone,
                    ["for-each-ref", "--format=%(upstream)", "refs/heads/feature"]
                ),
                "refs/remotes/origin/feature"
            );

            // `feature` tracks it now, and is checked out in that worktree.
            let again = fx.root().join("remote-clone-again");
            let error = add_worktree(
                &repo,
                &valid(s(&again)),
                &valid("refs/remotes/origin/feature"),
                None,
            )
            .await
            .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("'feature' is already used by worktree at"),
                "{error}"
            );

            let topic = fx.root().join("remote-clone-topic");
            add_worktree(
                &repo,
                &valid(s(&topic)),
                &valid("refs/remotes/origin/feature"),
                Some(&valid("topic")),
            )
            .await
            .unwrap();
        });
        assert_eq!(
            git(
                &clone,
                ["for-each-ref", "--format=%(upstream)", "refs/heads/topic"]
            ),
            "refs/remotes/origin/feature"
        );
    }

    #[test]
    fn refuses_a_branch_thats_checked_out_already_and_a_folder_thats_in_use() {
        let fx = Fixture::new();
        let path = create_committed_repo(&fx, "refused");
        smol::block_on(async {
            let repo = fx.repos.open("refused").await.unwrap();

            let main = fx.root().join("refused-main");
            let error = add_worktree(&repo, &valid(s(&main)), &valid("refs/heads/main"), None)
                .await
                .unwrap_err();
            assert!(
                error.to_string().contains(&format!(
                    "fatal: 'main' is already used by worktree at '{}'",
                    s(&path)
                )),
                "{error}"
            );
            let error = add_worktree(&repo, &valid(s(&path)), &valid("refs/heads/feature"), None)
                .await
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains(&format!("fatal: '{}' already exists", s(&path))),
                "{error}"
            );
            let x = fx.root().join("refused-x");
            let error = add_worktree(&repo, &valid(s(&x)), &valid("refs/heads/gone"), None)
                .await
                .unwrap_err();
            assert!(matches!(error, Error::InvalidInput(_)), "{error:?}");
            assert_eq!(error.to_string(), "refs/heads/gone isn't a branch.");
            let error = add_worktree(
                &repo,
                &valid(s(&x)),
                &valid("refs/heads/feature"),
                Some(&valid("a..b")),
            )
            .await
            .unwrap_err();
            assert_eq!(
                error.to_string(),
                "fatal: 'a..b' is not a valid branch name"
            );
        });
    }

    #[test]
    fn refuses_a_folder_or_a_name_thats_only_whitespace() {
        // Refused as the input's parsed: `add_worktree` takes a `Folder` and a `BranchName`.
        let error = Folder::parse(" ").unwrap_err();
        assert_eq!(error.to_string(), "The folder is empty.");
        let error = BranchName::parse(" ").unwrap_err();
        assert_eq!(error.to_string(), "The name is empty.");
    }

    // Removing a worktree.

    #[test]
    fn deletes_the_worktrees_folder_uncommitted_changes_and_all_keeping_its_branch() {
        let fx = Fixture::new();
        let path = create_committed_repo(&fx, "remove");
        let folder = fx.root().join("remove-feature");
        git(&path, ["worktree", "add", "-q", s(&folder), "feature"]);
        fs::write(folder.join("a.txt"), "changed\n").unwrap();
        fs::write(folder.join("new.txt"), "new\n").unwrap();

        smol::block_on(async {
            let repo = fx.repos.open("remove").await.unwrap();
            remove_worktree(&repo, s(&folder)).await.unwrap();
            assert!(!folder.exists());
            assert_eq!(
                summary(&fx, "remove").await,
                strs(&["remove* [refs/heads/main]"])
            );
        });
        assert!(!git(&path, ["rev-parse", "--verify", "refs/heads/feature"]).is_empty());
    }

    #[test]
    fn removes_one_whose_folder_is_gone_already() {
        let fx = Fixture::new();
        let path = create_committed_repo(&fx, "pruned");
        let folder = fx.root().join("pruned-feature");
        git(&path, ["worktree", "add", "-q", s(&folder), "feature"]);
        // Deleted outside git.
        fs::remove_dir_all(&folder).unwrap();

        smol::block_on(async {
            let repo = fx.repos.open("pruned").await.unwrap();
            let worktrees = list_worktrees(&repo).await.unwrap();
            assert!(worktrees[1].prunable.is_some());

            remove_worktree(&repo, s(&folder)).await.unwrap();
            assert_eq!(
                summary(&fx, "pruned").await,
                strs(&["pruned* [refs/heads/main]"])
            );
        });
    }

    #[test]
    fn refuses_the_main_worktree_the_one_open_and_one_thats_gone() {
        let fx = Fixture::new();
        let path = create_committed_repo(&fx, "blocked");
        let linked = fx.root().join("blocked-feature");
        git(&path, ["worktree", "add", "-q", s(&linked), "feature"]);
        fx.add_path("blocked-feature", &linked);

        smol::block_on(async {
            let blocked = fx.repos.open("blocked").await.unwrap();
            let error = remove_worktree(&blocked, s(&path)).await.unwrap_err();
            assert!(
                matches!(error, Error::WorktreeRemovalBlocked(_)),
                "{error:?}"
            );
            let feature = fx.repos.open("blocked-feature").await.unwrap();
            let error = remove_worktree(&feature, s(&linked)).await.unwrap_err();
            assert!(
                matches!(error, Error::WorktreeRemovalBlocked(_)),
                "{error:?}"
            );
            let gone = fx.root().join("blocked-gone");
            let error = remove_worktree(&blocked, s(&gone)).await.unwrap_err();
            assert!(matches!(error, Error::RepositoryChanged(_)), "{error:?}");
        });
        assert!(linked.exists());
    }

    #[test]
    fn refuses_a_locked_one_saying_so() {
        let fx = Fixture::new();
        let path = create_committed_repo(&fx, "locked");
        let linked = fx.root().join("locked-feature");
        git(&path, ["worktree", "add", "-q", s(&linked), "feature"]);
        git(&path, ["worktree", "lock", s(&linked), "--reason", "busy"]);

        smol::block_on(async {
            let repo = fx.repos.open("locked").await.unwrap();
            let error = remove_worktree(&repo, s(&linked)).await.unwrap_err();
            assert!(error.to_string().contains("lock reason: busy"), "{error}");
        });
        assert!(linked.exists());
    }

    #[test]
    fn orders_paths_regardless_of_case_then_lowercase_first() {
        let mut paths = ["/r/b", "/r/A", "/r/a", "/r/B", "/r/ab"];
        paths.sort_by_cached_key(|path| locale_key(path));
        assert_eq!(paths, ["/r/a", "/r/A", "/r/ab", "/r/b", "/r/B"]);
    }

    #[test]
    fn resolves_a_folder_as_node_does() {
        let base = Path::new("/repo/here");
        assert_eq!(resolve(base, "../x"), PathBuf::from("/repo/x"));
        assert_eq!(resolve(base, "./a/../b"), PathBuf::from("/repo/here/b"));
        assert_eq!(resolve(base, "/abs/./p"), PathBuf::from("/abs/p"));
        assert_eq!(resolve(base, "/../../x"), PathBuf::from("/x"));
    }
}
