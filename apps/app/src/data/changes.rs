//! Changing a repository: pulling, pushing, switching branches, stashing, committing… Each is a
//! method of [`Repository`], which runs git's part on the background executor and reloads what it
//! changed. Ported from `git/queries/`'s mutations.
//!
//! The ones the toolbar and the menus start (a pull, say) are the repository's operations: they
//! run one at a time, and the repository keeps whether each is running and why it last failed (see
//! [`Repository::run`]), so that outlives the view that started it: switching repositories shows
//! another one's, and the same one's again when switching back. Some run one of a group at a time
//! too, like the changes of the branches. The rest (committing, staging, fetching) are the view's
//! own, which keeps their state (see `super::mutation`); they return the task.

use std::path::PathBuf;

use futures::FutureExt as _;
use futures::channel::oneshot;
use gitto_git::features::branch::schema::{MergeOutcome, RebaseOutcome};
use gitto_git::features::conflicts::commands::ShownConflict;
use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::features::operation::schema::OperationKind;
use gitto_git::features::staging::discard::Discarded;
use gitto_git::input::{
    BranchName, BranchRef, FileInput, Folder, FullSha, Message, NonEmpty, Paths, RepositoryId,
};
use gitto_git::schema::FileStatus;
use gpui_kit::{AppContext as _, Context, Task};

use super::app_data::{AppData, AppPart};
use super::branch::{Merge, NewBranchFrom, Rebase};
use super::client::QueryError;
use super::commit::{NewCommit, Reword};
use super::conflicts::ConflictResolution;
use super::git::GitTask;
use super::repository::{Changed, Part, Repository};
use super::staging::{DiscardTarget, StageAction, StagingTarget};
use super::worktree::AddWorktreeInput;
use crate::services::Services;
use crate::state::router::{Route, Router};
use gitto_ui::git::diff_source::DiffSource;
use gitto_ui::git::patch::has_hunks;
use gitto_ui::git::status::{branch_name, is_checked_out};

/// What a change resolves to, once it's done: told to the view that started it, if it's still
/// there.
pub type Done<R = ()> = Option<oneshot::Receiver<Result<R, QueryError>>>;

pub const PULL: &str = "pull";
pub const PULL_BRANCH: &str = "pull-branch";
pub const PUSH: &str = "push";

/// The changes of the branches, which run one at a time (see [`Repository::branches_changing`]).
pub const CREATE_BRANCH: &str = "branch";
pub const SWITCH: &str = "switch";
pub const MERGE: &str = "merge";
pub const REBASE: &str = "rebase";
pub const DELETE_BRANCH: &str = "delete-branch";
/// Adding a worktree, which checks out a branch there, or makes one.
pub const ADD_WORKTREE: &str = "add-worktree";
const BRANCH_CHANGES: &[&str] = &[
    CREATE_BRANCH,
    SWITCH,
    MERGE,
    REBASE,
    DELETE_BRANCH,
    ADD_WORKTREE,
];

/// The stashes' changes, which run one at a time: the toolbar's pop names the stash it pops, which
/// has to be the newest. A pop of the newest, from the toolbar, and one of the stash picked in a
/// list are separate, to show each where it was started.
pub const STASH: &str = "stash";
pub const POP: &str = "pop";
pub const POP_PICKED: &str = "pop-picked";
pub const DROP_STASH: &str = "drop-stash";
const STASH_CHANGES: &[&str] = &[STASH, POP, POP_PICKED, DROP_STASH];

/// The worktrees' changes, which run one at a time.
pub const REMOVE_WORKTREE: &str = "remove-worktree";
pub const OPEN_WORKTREE: &str = "open-worktree";
const WORKTREE_CHANGES: &[&str] = &[ADD_WORKTREE, REMOVE_WORKTREE, OPEN_WORKTREE];

/// Continuing and aborting the operation under way.
pub const CONTINUE: &str = "continue";
pub const ABORT: &str = "abort";

impl Repository {
    /// `input`, checked as the UI hands it over (see `RepositoryGit::invalid`), as git's part of a
    /// change.
    fn checked<T, R: Send + 'static>(
        &self,
        input: gitto_git::Result<T>,
        action: impl FnOnce(T) -> GitTask<R>,
    ) -> GitTask<R> {
        match input {
            Ok(input) => action(input),
            Err(error) => {
                let error = self.git().invalid(error);
                async move { Err(error) }.boxed()
            }
        }
    }

    /// Runs `action`, git's part of a change, then reloads what's shown of the repository without
    /// waiting for it (also after a failure): the change is done, and its error shown, before the
    /// history has reloaded.
    fn then_reload<R: Send + 'static>(
        &mut self,
        action: GitTask<R>,
        cx: &mut Context<Self>,
    ) -> Task<Result<R, QueryError>> {
        let action = cx.background_spawn(action);
        cx.spawn(async move |this, cx| {
            let result = action.await;
            let _ = this.update(cx, |repository, cx| drop(repository.reload_all(cx)));
            result
        })
    }

    // Pulling and pushing.

    /// Pulls the current branch's upstream, unless it's pulling already (`usePull`). The
    /// repository's reloaded after, also after a failure: a pull that stopped at conflicts still
    /// brought in the upstream's commits.
    pub fn pull(&mut self, cx: &mut Context<Self>) -> Done {
        self.run(
            PULL,
            |repository, cx| repository.then_reload(repository.git().pull(None), cx),
            cx,
        )
    }

    /// Pulls a local branch that isn't checked out, by its short name, fast-forwarding it to its
    /// upstream without touching the working tree (`usePullBranch`). It counts as running until
    /// the repository has reloaded, also after a failure: its remote was fetched all the same.
    pub fn pull_branch(&mut self, branch: String, cx: &mut Context<Self>) -> Done {
        self.run(
            PULL_BRANCH,
            |repository, cx| {
                let git = repository.git().clone();
                let action =
                    repository.checked(NonEmpty::parse(branch), |branch| git.pull(Some(branch)));
                repository.reloading(action, cx)
            },
            cx,
        )
    }

    /// Pushes the current branch, or force-pushes it (`usePush`). A push turned down as the
    /// remote branch has commits the branch doesn't fails with an error `remote::push_rejection`
    /// tells apart. The remote-tracking branch moved, and so did how far ahead the branch is, so
    /// the repository's reloaded after, without waiting for it, as after a pull.
    pub fn push(&mut self, force: bool, cx: &mut Context<Self>) -> Done {
        self.run(
            PUSH,
            |repository, cx| repository.then_reload(repository.git().push(force), cx),
            cx,
        )
    }

    /// Fetches every remote (`useFetch`), then reloads the repository: the watcher would catch the
    /// new remote refs too, but reloading right away feels snappier. A view's own change.
    pub fn fetch(&mut self, cx: &mut Context<Self>) -> Task<Result<(), QueryError>> {
        let fetch = self.git().fetch();
        self.reloading(fetch, cx)
    }

    // Changing the branches.

    /// Whether one of the changes of the branches is running: creating one at HEAD, a switch of
    /// branches, a merge, a rebase, deleting one, or adding a worktree, which checks one out
    /// (`useBranchesChanging`). They run one at a time: a switch would switch away from the branch
    /// merged into before the user saw how the merge went, say, or check out a branch being
    /// deleted, which git then wouldn't delete.
    pub fn branches_changing(&self) -> bool {
        self.is_running(BRANCH_CHANGES)
    }

    /// Runs the change of the branches `name`, unless one is running.
    fn change_branches<R: 'static>(
        &mut self,
        name: &'static str,
        work: impl FnOnce(&mut Self, &mut Context<Self>) -> Task<Result<R, QueryError>>,
        cx: &mut Context<Self>,
    ) -> Done<R> {
        if self.branches_changing() {
            return None;
        }
        self.run(name, work, cx)
    }

    /// Creates a branch, by its name, at HEAD and switches to it, taking the uncommitted changes
    /// along (`useCreateBranch`). It counts as running until the repository has reloaded, so the
    /// toolbar names the new branch once it's done.
    pub fn create_branch(&mut self, name: String, cx: &mut Context<Self>) -> Done {
        self.change_branches(
            CREATE_BRANCH,
            |repository, cx| {
                let git = repository.git().clone();
                let action = repository.checked(BranchName::parse(name), |name| {
                    git.create_branch(name, None)
                });
                repository.reloading(action, cx)
            },
            cx,
        )
    }

    /// Switches to a branch, by its full ref name: for a remote one, the local branch tracking
    /// it, or a new one that does (`useSwitchBranch`). The uncommitted changes come along, or are
    /// kept in the stash when they conflict with it. It counts as running until the repository has
    /// reloaded, also after a failure: it may have switched and stashed. Switching to the
    /// checked-out branch does nothing, rather than reloading the whole repository.
    pub fn switch_branch(&mut self, reference: String, cx: &mut Context<Self>) -> Done {
        self.change_branches(
            SWITCH,
            |repository, cx| {
                let checked_out = repository
                    .status()
                    .data()
                    .is_some_and(|status| is_checked_out(&status.head, &reference));
                if checked_out {
                    return Task::ready(Ok(()));
                }
                let git = repository.git().clone();
                let action = repository.checked(BranchRef::parse(reference), |reference| {
                    git.switch_branch(reference)
                });
                repository.reloading(action, cx)
            },
            cx,
        )
    }

    /// Creates a branch at another (see [`NewBranchFrom`]) and switches to it, taking the
    /// uncommitted changes along, or keeping them in the stash when they conflict with it
    /// (`useCreateBranchFrom`). That's a switch of branches, like
    /// [`switch_branch`](Self::switch_branch)'s: it's shown as one.
    pub fn create_branch_from(&mut self, input: NewBranchFrom, cx: &mut Context<Self>) -> Done {
        self.change_branches(
            SWITCH,
            |repository, cx| {
                let git = repository.git().clone();
                let parsed = BranchName::parse(input.name)
                    .and_then(|name| Ok((name, BranchRef::parse(input.from)?)));
                let action =
                    repository.checked(parsed, |(name, from)| git.create_branch(name, Some(from)));
                repository.reloading(action, cx)
            },
            cx,
        )
    }

    /// Deletes a branch, by its full ref name (`useDeleteBranch`).
    pub fn delete_branch(&mut self, reference: String, cx: &mut Context<Self>) -> Done {
        self.change_branches(
            DELETE_BRANCH,
            |repository, cx| {
                let git = repository.git().clone();
                let action = repository.checked(BranchRef::parse(reference), |reference| {
                    git.delete_branch(reference)
                });
                repository.reloading(action, cx)
            },
            cx,
        )
    }

    /// Merges a branch into the checked-out one (see [`Merge`]); the merge's outcome, or why it
    /// failed (`useMergeBranch`). One that changed nothing fails, saying so: the branch had every
    /// commit of it already. One that stopped at conflicts goes through, as `Conflicts`, left for
    /// the user to resolve. It counts as running until the repository has reloaded, also after a
    /// failure: it may have stopped partway; it isn't reloaded after one that changed nothing.
    pub fn merge(&mut self, input: Merge, cx: &mut Context<Self>) -> Done<MergeOutcome> {
        self.change_branches(
            MERGE,
            |repository, cx| {
                let up_to_date = up_to_date_message(&input.into, &input.reference);
                let git = repository.git().clone();
                let parsed = BranchRef::parse(input.reference)
                    .and_then(|reference| Ok((reference, NonEmpty::parse(input.into)?)));
                let action = repository.checked(parsed, |(reference, into)| {
                    git.merge_branch(reference, into)
                });
                repository.integrating(
                    action,
                    |outcome| *outcome == MergeOutcome::UpToDate,
                    up_to_date,
                    cx,
                )
            },
            cx,
        )
    }

    /// Rebases the checked-out branch onto another (see [`Rebase`]); the rebase's outcome, or why
    /// it failed (`useRebaseBranch`). As a merge: one that changed nothing fails, saying so; one
    /// that stopped at conflicts goes through, as `Conflicts`, left for the user to resolve and
    /// continue.
    pub fn rebase(&mut self, input: Rebase, cx: &mut Context<Self>) -> Done<RebaseOutcome> {
        self.change_branches(
            REBASE,
            |repository, cx| {
                let up_to_date = up_to_date_message(&input.branch, &input.reference);
                let git = repository.git().clone();
                let parsed = BranchRef::parse(input.reference)
                    .and_then(|reference| Ok((reference, NonEmpty::parse(input.branch)?)));
                let action = repository.checked(parsed, |(reference, branch)| {
                    git.rebase_branch(reference, branch)
                });
                repository.integrating(
                    action,
                    |outcome| *outcome == RebaseOutcome::UpToDate,
                    up_to_date,
                    cx,
                )
            },
            cx,
        )
    }

    /// Runs `action`, a merge or a rebase, then reloads the repository, also after a failure: it
    /// may have stopped partway. Not after one that changed nothing (`is_up_to_date`), which fails
    /// instead, saying `up_to_date`: the branch had every commit already.
    pub(crate) fn integrating<T: Send + 'static>(
        &mut self,
        action: GitTask<T>,
        is_up_to_date: impl FnOnce(&T) -> bool + 'static,
        up_to_date: String,
        cx: &mut Context<Self>,
    ) -> Task<Result<T, QueryError>> {
        let action = cx.background_spawn(action);
        cx.spawn(async move |this, cx| {
            let outcome = action.await;
            if let Ok(outcome) = &outcome
                && is_up_to_date(outcome)
            {
                return Err(QueryError::msg(up_to_date));
            }
            if let Ok(reloading) = this.update(cx, |repository, cx| repository.reload_all(cx)) {
                reloading.await;
            }
            outcome
        })
    }

    // Stashing.

    /// Whether one of the stashes' changes is running: one runs at a time.
    pub fn stashes_changing(&self) -> bool {
        self.is_running(STASH_CHANGES)
    }

    /// Runs the stashes' change `name`, unless one is running. Each counts as running until the
    /// repository has reloaded, also after a failure (a pop can conflict), so the stashes and
    /// changes on show are never ones from before it.
    fn change_stashes(
        &mut self,
        name: &'static str,
        action: impl FnOnce(&Self) -> GitTask<()>,
        cx: &mut Context<Self>,
    ) -> Done {
        if self.stashes_changing() {
            return None;
        }
        self.run(
            name,
            |repository, cx| {
                let action = action(repository);
                repository.reloading(action, cx)
            },
            cx,
        )
    }

    /// Stashes every change (`useStashActions`'s `stash`).
    pub fn stash(&mut self, cx: &mut Context<Self>) -> Done {
        self.change_stashes(STASH, |repository| repository.git().push_stash(), cx)
    }

    /// Pops a stash, by its SHA: the newest, from the toolbar ([`POP`]), or the one picked in a
    /// list ([`POP_PICKED`]).
    pub fn pop_stash(&mut self, name: &'static str, sha: String, cx: &mut Context<Self>) -> Done {
        self.change_stashes(
            name,
            |repository| {
                let git = repository.git().clone();
                repository.checked(FullSha::parse(sha), |sha| git.pop_stash(sha))
            },
            cx,
        )
    }

    /// Drops a stash, by its SHA.
    pub fn drop_stash(&mut self, sha: String, cx: &mut Context<Self>) -> Done {
        self.change_stashes(
            DROP_STASH,
            |repository| {
                let git = repository.git().clone();
                repository.checked(FullSha::parse(sha), |sha| git.drop_stash(sha))
            },
            cx,
        )
    }

    // The worktrees.

    /// Whether one of the worktrees' changes is running: one runs at a time.
    pub fn worktrees_changing(&self) -> bool {
        self.is_running(WORKTREE_CHANGES)
    }

    /// Whether adding a worktree can't run now: one of the worktrees' changes is running, or a
    /// change of the branches is, as adding one checks one out.
    pub fn add_worktree_blocked(&self) -> bool {
        self.worktrees_changing() || self.branches_changing()
    }

    /// Adds a worktree (see [`AddWorktreeInput`]). It counts as running until the repository has
    /// reloaded, so the worktrees on show are never ones from before.
    pub fn add_worktree(&mut self, input: AddWorktreeInput, cx: &mut Context<Self>) -> Done {
        if self.add_worktree_blocked() {
            return None;
        }
        self.run(
            ADD_WORKTREE,
            |repository, cx| {
                let git = repository.git().clone();
                let parsed = Folder::parse(input.path).and_then(|folder| {
                    let branch = BranchRef::parse(input.branch)?;
                    let new_branch = input.new_branch.map(BranchName::parse).transpose()?;
                    Ok((folder, branch, new_branch))
                });
                let action = repository.checked(parsed, |(folder, branch, new_branch)| {
                    git.add_worktree(folder, branch, new_branch)
                });
                repository.reloading(action, cx)
            },
            cx,
        )
    }

    /// Removes a worktree, by its path, and the repository it was opened as in Gitto, if it was,
    /// as its folder is gone. It counts as running until the repository has reloaded.
    pub fn remove_worktree(&mut self, path: String, cx: &mut Context<Self>) -> Done {
        if self.worktrees_changing() {
            return None;
        }
        self.run(
            REMOVE_WORKTREE,
            |repository, cx| {
                let removing = cx.background_spawn(repository.git().remove_worktree(path.clone()));
                let source = Services::global(cx).repository_source().clone();
                let data = AppData::global(cx);
                cx.spawn(async move |this, cx| {
                    let result = async {
                        removing.await?;
                        let loaded = cx.update(|cx| data.read(cx).repositories().data().cloned());
                        let repositories = match loaded {
                            Some(repositories) => repositories,
                            None => source.list().await?,
                        };
                        let Some(opened) = repositories
                            .into_iter()
                            .find(|repository| repository.path == path)
                        else {
                            return Ok(());
                        };
                        source.remove(opened.id.clone()).await?;
                        let reloaded = cx.update(|cx| {
                            super::repository::Repositories::close(&opened.id, cx);
                            data.update(cx, |data, cx| data.reload(AppPart::Repositories, cx))
                        });
                        if let Some(reloaded) = reloaded {
                            let _ = reloaded.await;
                        }
                        Ok::<_, QueryError>(())
                    }
                    .await;
                    if let Ok(reloading) =
                        this.update(cx, |repository, cx| repository.reload_all(cx))
                    {
                        reloading.await;
                    }
                    result
                })
            },
            cx,
        )
    }

    /// Opens a worktree of the repository, by its path: adds it to Gitto's repositories, as one of
    /// its own, or finds it there, and shows it (`useOpenWorktree`). Whether that's running, and
    /// why it last failed, wherever it was started from (a worktree's menu, or a branch's).
    pub fn open_worktree(&mut self, path: String, cx: &mut Context<Self>) -> Done {
        if self.worktrees_changing() {
            return None;
        }
        self.run(
            OPEN_WORKTREE,
            |_, cx| {
                let adding = Services::global(cx)
                    .repository_source()
                    .add(PathBuf::from(path));
                let data = AppData::global(cx);
                cx.spawn(async move |_, cx| {
                    let repository = adding.await?;
                    let reloaded = cx.update(|cx| {
                        data.update(cx, |data, cx| data.reload(AppPart::Repositories, cx))
                    });
                    if let Some(reloaded) = reloaded {
                        let _ = reloaded.await;
                    }
                    let id = RepositoryId::parse(repository.id)?;
                    cx.update(|cx| Router::navigate(Route::Repository(id), cx));
                    Ok(())
                })
            },
            cx,
        )
    }

    // The operation under way.

    /// Continues the merge, rebase, cherry-pick or revert under way. It counts as running until
    /// the repository has reloaded, so the operation on show is never the one from before: a
    /// rebase that goes on stops at the next commit's conflicts, say.
    pub fn continue_operation(&mut self, kind: OperationKind, cx: &mut Context<Self>) -> Done {
        self.run(
            CONTINUE,
            |repository, cx| {
                let action = repository.git().continue_operation(kind);
                repository.reloading(action, cx)
            },
            cx,
        )
    }

    /// Aborts the merge, rebase, cherry-pick or revert under way, as continuing does.
    pub fn abort_operation(&mut self, kind: OperationKind, cx: &mut Context<Self>) -> Done {
        self.run(
            ABORT,
            |repository, cx| {
                let action = repository.git().abort_operation(kind);
                repository.reloading(action, cx)
            },
            cx,
        )
    }

    // Committing: a view's own changes.

    /// Commits what's staged (see [`NewCommit`]; `useCreateCommit`).
    pub fn create_commit(
        &mut self,
        commit: NewCommit,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), QueryError>> {
        let git = self.git().clone();
        let parsed = Message::parse(commit.message)
            .and_then(|message| Ok((message, commit.amend.map(FullSha::parse).transpose()?)));
        let action = self.checked(parsed, |(message, amend)| git.create_commit(message, amend));
        self.refresh_after_commit(action, cx)
    }

    /// Rewrites the message of HEAD's commit, leaving what's staged out of it; resolves to the
    /// rewritten commit's SHA once the repository has reloaded (`useRewordCommit`).
    pub fn reword_commit(
        &mut self,
        reword: Reword,
        cx: &mut Context<Self>,
    ) -> Task<Result<String, QueryError>> {
        let git = self.git().clone();
        let parsed =
            FullSha::parse(reword.sha).and_then(|sha| Ok((sha, Message::parse(reword.message)?)));
        let action = self.checked(parsed, |(sha, message)| git.reword_commit(sha, message));
        self.refresh_after_commit(action, cx)
    }

    /// Runs a commit's `action`, then reloads what it changed: the whole repository once it's
    /// done, waited for; after a failure, the uncommitted changes only (`refreshAfterCommit`).
    fn refresh_after_commit<R: Send + 'static>(
        &mut self,
        action: GitTask<R>,
        cx: &mut Context<Self>,
    ) -> Task<Result<R, QueryError>> {
        let action = cx.background_spawn(action);
        cx.spawn(async move |this, cx| {
            let result = action.await;
            match &result {
                Ok(_) => {
                    if let Ok(reloading) =
                        this.update(cx, |repository, cx| repository.reload_all(cx))
                    {
                        reloading.await;
                    }
                }
                // A failure saying HEAD has moved means the status is behind; reloaded without
                // waiting for it, so the error shows right away.
                Err(_) => {
                    let _ =
                        this.update(cx, |repository, cx| drop(repository.reload_uncommitted(cx)));
                }
            }
            result
        })
    }

    // Staging: a view's own changes. Each reloads the uncommitted changes after, without waiting:
    // the watcher would catch it too, but reloading right away feels snappier.

    /// Runs `action`, a change to the uncommitted changes, then reloads them without waiting.
    fn then_reload_uncommitted(
        &mut self,
        action: GitTask<()>,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), QueryError>> {
        let action = cx.background_spawn(action);
        cx.spawn(async move |this, cx| {
            let result = action.await;
            let _ = this.update(cx, |repository, cx| drop(repository.reload_uncommitted(cx)));
            result
        })
    }

    /// Stages files, or all of them (`useStage`).
    pub fn stage(
        &mut self,
        target: StagingTarget,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), QueryError>> {
        let git = self.git().clone();
        let action = match target {
            StagingTarget::All => git.stage_all(),
            StagingTarget::Paths(paths) => {
                self.checked(Paths::parse(paths), |paths| git.stage(paths))
            }
        };
        self.then_reload_uncommitted(action, cx)
    }

    /// Unstages files, or all of them (`useUnstage`).
    pub fn unstage(
        &mut self,
        target: StagingTarget,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), QueryError>> {
        let git = self.git().clone();
        let action = match target {
            StagingTarget::All => git.unstage_all(),
            StagingTarget::Paths(paths) => {
                self.checked(Paths::parse(paths), |paths| git.unstage(paths))
            }
        };
        self.then_reload_uncommitted(action, cx)
    }

    /// Discards changes, which are lost: a file's unstaged ones, or staged ones and all, by both
    /// its paths if it's renamed; or every change, deleting untracked files too (`useDiscard`). A
    /// file that changed since its list was read isn't discarded (see `discard`).
    pub fn discard(
        &mut self,
        target: DiscardTarget,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), QueryError>> {
        let action = match target {
            DiscardTarget::All => self.git().discard_all(),
            DiscardTarget::File { side, file } => self.git().discard(Discarded {
                path: file.path,
                orig_path: file.orig_path,
                status: file.status,
                side,
            }),
        };
        self.then_reload_uncommitted(action, cx)
    }

    /// Stages or unstages a whole file at `paths` (a renamed one by both).
    pub fn stage_file(
        &mut self,
        action: StageAction,
        paths: Vec<String>,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), QueryError>> {
        let git = self.git().clone();
        let action = self.checked(Paths::parse(paths), |paths| match action {
            StageAction::Stage => git.stage(paths),
            StageAction::Unstage => git.unstage(paths),
        });
        self.then_reload_uncommitted(action, cx)
    }

    /// Stages or unstages lines of `file`'s changes, picked from its patch named by `version`;
    /// resolves to the file's patch they leave (`useStageLines`). That takes the place of the one
    /// on show at once, if it has changes left: one without is left for the view to move on from,
    /// rather than said to have none (see `has_hunks`). The rest of the uncommitted changes, like
    /// the lists, are reloaded without waiting for them. If the lines couldn't be staged, it
    /// settles once all of it is reloaded: the patch they were picked from may be out of date, and
    /// isn't picked from again until it isn't.
    pub fn stage_lines(
        &mut self,
        action: StageAction,
        file: ChangedFile,
        version: String,
        lines: gitto_git::features::staging::schema::LineSelection,
        cx: &mut Context<Self>,
    ) -> Task<Result<String, QueryError>> {
        let path = file.path.clone();
        let input = FileInput {
            path: file.path.clone(),
            orig_path: file.orig_path.clone(),
        };
        let staging = match action {
            StageAction::Stage => {
                let untracked = file.status == FileStatus::Untracked;
                self.git().stage_lines(input, untracked, version, lines)
            }
            StageAction::Unstage => self.git().unstage_lines(input, version, lines),
        };
        let staging = cx.background_spawn(staging);
        cx.spawn(async move |this, cx| match staging.await {
            Ok(patch) => {
                let _ = this.update(cx, |repository, cx| {
                    repository.show_patch_left(action, &path, &patch, cx)
                });
                Ok(patch)
            }
            Err(error) => {
                if let Ok(reloading) =
                    this.update(cx, |repository, cx| repository.reload_uncommitted(cx))
                {
                    reloading.await;
                }
                Err(error)
            }
        })
    }

    /// Shows `patch`, what staging or unstaging lines of the file at `path` left of its changes,
    /// in place of the patch they were picked from, if it has changes left; and reloads the rest
    /// of the uncommitted changes, without waiting for them.
    fn show_patch_left(
        &mut self,
        action: StageAction,
        path: &str,
        patch: &str,
        cx: &mut Context<Self>,
    ) {
        let source = match action {
            StageAction::Stage => DiffSource::Unstaged,
            StageAction::Unstage => DiffSource::Staged,
        };
        let shown = has_hunks(patch) && self.replace_patch(&source, path, patch);
        drop(self.reload_uncommitted_but(shown.then_some((source, path.to_owned())), cx));
        cx.emit(Changed(Part::Patches));
        cx.notify();
    }

    // Resolving conflicts: a view's own change.

    /// Resolves a conflicted file whole (see [`ConflictResolution`]; `useResolveFile`). Settles
    /// once that's done, rather than once the uncommitted changes are reloaded after, so the view
    /// can move on at once.
    pub fn resolve_file(
        &mut self,
        path: String,
        resolution: ConflictResolution,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), QueryError>> {
        let git = self.git().clone();
        let action = self.checked(NonEmpty::parse(path), |path| match resolution {
            ConflictResolution::Keep {
                side,
                sides,
                version,
            } => git.keep_side(path, side, ShownConflict { sides, version }),
            ConflictResolution::Mark {
                version,
                with_markers,
            } => git.mark_resolved(path, version, with_markers),
        });
        self.then_reload_uncommitted(action, cx)
    }
}

/// Why a merge or a rebase that changed nothing fails: `branch`, as the user saw it, had every
/// commit of the branch `reference`, by its full ref name, already.
fn up_to_date_message(branch: &str, reference: &str) -> String {
    format!(
        "Already up to date: {branch} has every commit of {}.",
        branch_name(reference)
    )
}
