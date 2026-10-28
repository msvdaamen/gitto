//! Changing a repository: pulling, pushing, switching branches, stashing, committing… Each is a
//! method of [`Repository`], which runs git's part on the background executor and reloads what it
//! changed. Ported from `git/queries/`'s mutations.
//!
//! The ones the toolbar and the menus start (a pull, say) are the repository's [`Change`]s: they
//! run one at a time, and the repository keeps whether each is running and why it last failed (see
//! [`Repository::run`]), so that outlives the view that started it: switching repositories shows
//! another one's, and the same one's again when switching back. Some run one of a group at a time
//! too, like the changes of the branches. The rest (committing, staging) are the view's own, which
//! keeps their state (see `super::mutation`); they return the task.

use std::path::PathBuf;

use futures::FutureExt as _;
use futures::channel::oneshot;
use gitto_git::Discarded;
use gitto_git::ShownConflict;
use gitto_git::features::branch::schema::{MergeOutcome, RebaseOutcome};
use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::features::operation::schema::OperationKind;
use gitto_git::input::{BranchName, BranchRef, FileInput, FullSha, NonEmpty, Paths, RepositoryId};
use gitto_git::schema::FileStatus;
use gpui_kit::{AppContext as _, Context, Task};

use super::app_data::{AppData, AppPart};
use super::branch::{Merge, NewBranchFrom, Rebase};
use super::commit::{NewCommit, Reword};
use super::conflicts::ConflictResolution;
use super::error::DataError;
use super::git::GitTask;
use super::repository::{Changed, Part, Repository};
use super::staging::{DiscardTarget, StageAction, StagingTarget};
use super::worktree::AddWorktreeInput;
use crate::services::Services;
use gitto_ui_logic::git::diff_source::DiffSource;
use gitto_ui_logic::git::patch::has_hunks;
use gitto_ui_logic::git::status::{branch_name, is_checked_out};

/// What a change resolves to, once it's done: told to the view that started it, if it's still
/// there.
pub type Done<R = ()> = Option<oneshot::Receiver<Result<R, DataError>>>;

/// A change to a repository that the toolbar or a menu starts. Each runs one at a time, and the
/// repository keeps whether it's running and why it last failed (see [`Repository::run`]); some
/// run one of a group at a time too (see [`ChangeGroup`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Change {
    /// Fetching every remote.
    Fetch,
    /// Pulling the checked-out branch.
    Pull,
    /// Pulling a local branch that isn't checked out.
    PullBranch,
    Push,
    /// Creating a branch at HEAD.
    CreateBranch,
    /// Switching branches, or creating one at another and switching to it.
    Switch,
    Merge,
    Rebase,
    DeleteBranch,
    /// Adding a worktree, which checks out a branch there, or makes one.
    AddWorktree,
    Stash,
    /// Popping a stash, from where it was started.
    Pop(PopFrom),
    DropStash,
    RemoveWorktree,
    OpenWorktree,
    /// Continuing the merge, rebase, cherry-pick or revert under way.
    Continue,
    /// Aborting it.
    Abort,
}

/// Where a stash was popped from: the toolbar, which pops the newest, or a list, which pops the
/// one picked. Each shows its own pop, where it was started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PopFrom {
    Toolbar,
    Picked,
}

/// Changes that run one of them at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeGroup {
    /// The changes of the branches (see [`Repository::branches_changing`]).
    Branches,
    Stashes,
    Worktrees,
}

impl Change {
    /// Whether it's one of `group`: adding a worktree is one of the branches' and of the
    /// worktrees'.
    pub fn is_in(self, group: ChangeGroup) -> bool {
        match group {
            ChangeGroup::Branches => matches!(
                self,
                Change::CreateBranch
                    | Change::Switch
                    | Change::Merge
                    | Change::Rebase
                    | Change::DeleteBranch
                    | Change::AddWorktree
            ),
            ChangeGroup::Stashes => {
                matches!(self, Change::Stash | Change::Pop(_) | Change::DropStash)
            }
            ChangeGroup::Worktrees => matches!(
                self,
                Change::AddWorktree | Change::RemoveWorktree | Change::OpenWorktree
            ),
        }
    }
}

/// What a change reloads once git's part of it is done (see [`Repository::after`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Then {
    /// Nothing: it changed nothing.
    Settle,
    /// What's shown of `Scope`, settling at once: the change is done, and its error shown,
    /// before that's reloaded.
    Reload(Scope),
    /// What's shown of `Scope`, settling once that's reloaded, so what's shown is what the change
    /// left before it can be run again.
    AwaitReload(Scope),
}

/// What of a repository a change reloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    /// What's shown of all of it (see [`Repository::reload_all`]).
    All,
    /// What's shown of the uncommitted changes (see [`Repository::reload_uncommitted`]).
    Uncommitted,
}

impl Repository {
    /// `input`, checked as the UI hands it over (see `RepositoryGit::invalid`), as git's part of a
    /// change: the paths of files, from their lists.
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

    /// Runs `action`, git's part of a change, on the background executor, then reloads what
    /// `then` says for its outcome (see [`Then`]); resolves to the outcome, once that's reloaded
    /// if `then` says to wait.
    pub(crate) fn after<R: Send + 'static>(
        &mut self,
        action: GitTask<R>,
        then: impl FnOnce(&Result<R, DataError>) -> Then + 'static,
        cx: &mut Context<Self>,
    ) -> Task<Result<R, DataError>> {
        let action = cx.background_spawn(action);
        cx.spawn(async move |this, cx| {
            let result = action.await;
            let (scope, wait) = match then(&result) {
                Then::Settle => return result,
                Then::Reload(scope) => (scope, false),
                Then::AwaitReload(scope) => (scope, true),
            };
            let reloading = this.update(cx, |repository, cx| match scope {
                Scope::All => repository.reload_all(cx),
                Scope::Uncommitted => repository.reload_uncommitted(cx),
            });
            if wait && let Ok(reloading) = reloading {
                reloading.await;
            }
            result
        })
    }

    // Pulling and pushing.

    /// Pulls the current branch's upstream, unless it's pulling already (`usePull`). The
    /// repository's reloaded after, also after a failure: a pull that stopped at conflicts still
    /// brought in the upstream's commits.
    pub fn pull(&mut self, cx: &mut Context<Self>) -> Done {
        self.run(
            Change::Pull,
            |repository, cx| {
                let pull = repository.git().pull(None);
                repository.after(pull, |_| Then::Reload(Scope::All), cx)
            },
            cx,
        )
    }

    /// Pulls a local branch that isn't checked out, by its short name, fast-forwarding it to its
    /// upstream without touching the working tree (`usePullBranch`). It counts as running until
    /// the repository has reloaded, also after a failure: its remote was fetched all the same.
    pub fn pull_branch(&mut self, branch: NonEmpty, cx: &mut Context<Self>) -> Done {
        self.run(
            Change::PullBranch,
            |repository, cx| {
                let action = repository.git().pull(Some(branch));
                repository.after(action, |_| Then::AwaitReload(Scope::All), cx)
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
            Change::Push,
            |repository, cx| {
                let push = repository.git().push(force);
                repository.after(push, |_| Then::Reload(Scope::All), cx)
            },
            cx,
        )
    }

    /// Fetches every remote (`useFetch`), unless it's fetching already, then reloads the
    /// repository: the watcher would catch the new remote refs too, but reloading right away feels
    /// snappier. It counts as running until the repository has reloaded.
    pub fn fetch(&mut self, cx: &mut Context<Self>) -> Done {
        self.run(
            Change::Fetch,
            |repository, cx| {
                let fetch = repository.git().fetch();
                repository.after(fetch, |_| Then::AwaitReload(Scope::All), cx)
            },
            cx,
        )
    }

    // Changing the branches.

    /// Whether one of the changes of the branches is running: creating one at HEAD, a switch of
    /// branches, a merge, a rebase, deleting one, or adding a worktree, which checks one out
    /// (`useBranchesChanging`). They run one at a time: a switch would switch away from the branch
    /// merged into before the user saw how the merge went, say, or check out a branch being
    /// deleted, which git then wouldn't delete.
    pub fn branches_changing(&self) -> bool {
        self.is_changing(ChangeGroup::Branches)
    }

    /// Runs the change of the branches `change`, unless one is running.
    fn change_branches<R: 'static>(
        &mut self,
        change: Change,
        work: impl FnOnce(&mut Self, &mut Context<Self>) -> Task<Result<R, DataError>>,
        cx: &mut Context<Self>,
    ) -> Done<R> {
        if self.branches_changing() {
            return None;
        }
        self.run(change, work, cx)
    }

    /// Creates a branch, by its name, at HEAD and switches to it, taking the uncommitted changes
    /// along (`useCreateBranch`). It counts as running until the repository has reloaded, so the
    /// toolbar names the new branch once it's done.
    pub fn create_branch(&mut self, name: BranchName, cx: &mut Context<Self>) -> Done {
        self.change_branches(
            Change::CreateBranch,
            |repository, cx| {
                let action = repository.git().create_branch(name, None);
                repository.after(action, |_| Then::AwaitReload(Scope::All), cx)
            },
            cx,
        )
    }

    /// Switches to a branch, by its full ref name: for a remote one, the local branch tracking
    /// it, or a new one that does (`useSwitchBranch`). The uncommitted changes come along, or are
    /// kept in the stash when they conflict with it. It counts as running until the repository has
    /// reloaded, also after a failure: it may have switched and stashed. Switching to the
    /// checked-out branch does nothing, rather than reloading the whole repository.
    pub fn switch_branch(&mut self, reference: BranchRef, cx: &mut Context<Self>) -> Done {
        self.change_branches(
            Change::Switch,
            |repository, cx| {
                let checked_out = repository
                    .status()
                    .data()
                    .is_some_and(|status| is_checked_out(&status.head, &reference));
                if checked_out {
                    return Task::ready(Ok(()));
                }
                let action = repository.git().switch_branch(reference);
                repository.after(action, |_| Then::AwaitReload(Scope::All), cx)
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
            Change::Switch,
            |repository, cx| {
                let action = repository.git().create_branch(input.name, Some(input.from));
                repository.after(action, |_| Then::AwaitReload(Scope::All), cx)
            },
            cx,
        )
    }

    /// Deletes a branch, by its full ref name (`useDeleteBranch`).
    pub fn delete_branch(&mut self, reference: BranchRef, cx: &mut Context<Self>) -> Done {
        self.change_branches(
            Change::DeleteBranch,
            |repository, cx| {
                let action = repository.git().delete_branch(reference);
                repository.after(action, |_| Then::AwaitReload(Scope::All), cx)
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
            Change::Merge,
            |repository, cx| {
                let up_to_date = up_to_date_message(&input.into, &input.reference);
                let action = repository.git().merge_branch(input.reference, input.into);
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
            Change::Rebase,
            |repository, cx| {
                let up_to_date = up_to_date_message(&input.branch, &input.reference);
                let action = repository
                    .git()
                    .rebase_branch(input.reference, input.branch);
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
        is_up_to_date: impl FnOnce(&T) -> bool + Send + 'static,
        up_to_date: String,
        cx: &mut Context<Self>,
    ) -> Task<Result<T, DataError>> {
        let action = async move {
            let outcome = action.await?;
            let changed_nothing = is_up_to_date(&outcome);
            Ok((outcome, changed_nothing))
        }
        .boxed();
        let integrating = self.after(
            action,
            |result| match result {
                Ok((_, true)) => Then::Settle,
                _ => Then::AwaitReload(Scope::All),
            },
            cx,
        );
        cx.spawn(async move |_, _| match integrating.await? {
            (_, true) => Err(DataError::msg(up_to_date)),
            (outcome, false) => Ok(outcome),
        })
    }

    // Stashing.

    /// Whether one of the stashes' changes is running: one runs at a time.
    pub fn stashes_changing(&self) -> bool {
        self.is_changing(ChangeGroup::Stashes)
    }

    /// Runs the stashes' change `change`, unless one is running. Each counts as running until the
    /// repository has reloaded, also after a failure (a pop can conflict), so the stashes and
    /// changes on show are never ones from before it.
    fn change_stashes(
        &mut self,
        change: Change,
        action: impl FnOnce(&Self) -> GitTask<()>,
        cx: &mut Context<Self>,
    ) -> Done {
        if self.stashes_changing() {
            return None;
        }
        self.run(
            change,
            |repository, cx| {
                let action = action(repository);
                repository.after(action, |_| Then::AwaitReload(Scope::All), cx)
            },
            cx,
        )
    }

    /// Stashes every change (`useStashActions`'s `stash`).
    pub fn stash(&mut self, cx: &mut Context<Self>) -> Done {
        self.change_stashes(
            Change::Stash,
            |repository| repository.git().push_stash(),
            cx,
        )
    }

    /// Pops a stash, by its SHA: the newest, from the toolbar, or the one picked in a list.
    pub fn pop_stash(&mut self, from: PopFrom, sha: FullSha, cx: &mut Context<Self>) -> Done {
        self.change_stashes(
            Change::Pop(from),
            |repository| repository.git().pop_stash(sha),
            cx,
        )
    }

    /// Drops a stash, by its SHA.
    pub fn drop_stash(&mut self, sha: FullSha, cx: &mut Context<Self>) -> Done {
        self.change_stashes(
            Change::DropStash,
            |repository| repository.git().drop_stash(sha),
            cx,
        )
    }

    // The worktrees.

    /// Whether one of the worktrees' changes is running: one runs at a time.
    pub fn worktrees_changing(&self) -> bool {
        self.is_changing(ChangeGroup::Worktrees)
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
            Change::AddWorktree,
            |repository, cx| {
                let action =
                    repository
                        .git()
                        .add_worktree(input.path, input.branch, input.new_branch);
                repository.after(action, |_| Then::AwaitReload(Scope::All), cx)
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
            Change::RemoveWorktree,
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
                        Ok::<_, DataError>(())
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
    /// its own, or finds it there; resolves to its id, once the list's reloaded, for the view that
    /// opened it to show it (`useOpenWorktree`). Whether that's running, and why it last failed,
    /// wherever it was started from (a worktree's menu, or a branch's).
    pub fn open_worktree(&mut self, path: String, cx: &mut Context<Self>) -> Done<RepositoryId> {
        if self.worktrees_changing() {
            return None;
        }
        self.run(
            Change::OpenWorktree,
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
                    Ok(RepositoryId::parse(repository.id)?)
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
            Change::Continue,
            |repository, cx| {
                let action = repository.git().continue_operation(kind);
                repository.after(action, |_| Then::AwaitReload(Scope::All), cx)
            },
            cx,
        )
    }

    /// Aborts the merge, rebase, cherry-pick or revert under way, as continuing does.
    pub fn abort_operation(&mut self, kind: OperationKind, cx: &mut Context<Self>) -> Done {
        self.run(
            Change::Abort,
            |repository, cx| {
                let action = repository.git().abort_operation(kind);
                repository.after(action, |_| Then::AwaitReload(Scope::All), cx)
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
    ) -> Task<Result<(), DataError>> {
        let action = self.git().create_commit(commit.message, commit.amend);
        self.after(action, after_commit, cx)
    }

    /// Rewrites the message of HEAD's commit, leaving what's staged out of it; resolves to the
    /// rewritten commit's SHA once the repository has reloaded (`useRewordCommit`).
    pub fn reword_commit(
        &mut self,
        reword: Reword,
        cx: &mut Context<Self>,
    ) -> Task<Result<String, DataError>> {
        let action = self.git().reword_commit(reword.sha, reword.message);
        self.after(action, after_commit, cx)
    }

    // Staging: a view's own changes. Each reloads the uncommitted changes after: the watcher
    // would catch it too, but reloading right away feels snappier.

    /// Stages files, or all of them (`useStage`).
    pub fn stage(
        &mut self,
        target: StagingTarget,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), DataError>> {
        let git = self.git().clone();
        let action = match target {
            StagingTarget::All => git.stage_all(),
            StagingTarget::Paths(paths) => {
                self.checked(Paths::parse(paths), |paths| git.stage(paths))
            }
        };
        self.after(action, after_staging, cx)
    }

    /// Unstages files, or all of them (`useUnstage`).
    pub fn unstage(
        &mut self,
        target: StagingTarget,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), DataError>> {
        let git = self.git().clone();
        let action = match target {
            StagingTarget::All => git.unstage_all(),
            StagingTarget::Paths(paths) => {
                self.checked(Paths::parse(paths), |paths| git.unstage(paths))
            }
        };
        self.after(action, after_staging, cx)
    }

    /// Discards changes, which are lost: a file's unstaged ones, or staged ones and all, by both
    /// its paths if it's renamed; or every change, deleting untracked files too (`useDiscard`). A
    /// file that changed since its list was read isn't discarded (see `discard`).
    pub fn discard(
        &mut self,
        target: DiscardTarget,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), DataError>> {
        let action = match target {
            DiscardTarget::All => self.git().discard_all(),
            DiscardTarget::File { side, file } => self.git().discard(Discarded {
                path: file.path,
                orig_path: file.orig_path,
                status: file.status,
                side,
            }),
        };
        self.after(action, after_staging, cx)
    }

    /// Stages or unstages a whole file at `paths` (a renamed one by both).
    pub fn stage_file(
        &mut self,
        action: StageAction,
        paths: Vec<String>,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), DataError>> {
        let git = self.git().clone();
        let action = self.checked(Paths::parse(paths), |paths| match action {
            StageAction::Stage => git.stage(paths),
            StageAction::Unstage => git.unstage(paths),
        });
        self.after(action, |_| Then::Reload(Scope::Uncommitted), cx)
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
    ) -> Task<Result<String, DataError>> {
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
    ) -> Task<Result<(), DataError>> {
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
        self.after(action, |_| Then::Reload(Scope::Uncommitted), cx)
    }
}

/// What a commit reloads: the whole repository once it's done, waited for; after a failure, the
/// uncommitted changes only (`refreshAfterCommit`). A failure saying HEAD has moved means the
/// status is behind; it's reloaded without waiting for it, so the error shows right away.
fn after_commit<R>(result: &Result<R, DataError>) -> Then {
    match result {
        Ok(_) => Then::AwaitReload(Scope::All),
        Err(_) => Then::Reload(Scope::Uncommitted),
    }
}

/// What a change to the uncommitted changes reloads: them, settling once they have
/// (`useStagingMutation`), so what's shown is what it left before it can be run again; nothing
/// after a failure.
fn after_staging(result: &Result<(), DataError>) -> Then {
    match result {
        Ok(()) => Then::AwaitReload(Scope::Uncommitted),
        Err(_) => Then::Settle,
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

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use futures::FutureExt as _;
    use gitto_git::features::refs::schema::Ref;
    use gitto_git::features::status::schema::{Head, StatusCounts, Uncommitted, WorkingTreeFiles};
    use gpui_kit::{Entity, TestAppContext};

    use super::*;
    use crate::data::Field;
    use crate::data::git::RepositoryGit;

    fn repository_id() -> RepositoryId {
        RepositoryId::parse("01890a5d-ac96-774b-bcce-b302099a8057").unwrap()
    }

    /// A repository's git that merges as `merged` says, deletes a branch when the test says, and
    /// counts how often the branches are loaded: once for each reload of the repository.
    #[derive(Default)]
    struct Git {
        merged: Cell<Option<MergeOutcome>>,
        deleting: RefCell<Option<oneshot::Receiver<()>>>,
        refs: Cell<usize>,
    }

    impl RepositoryGit for Git {
        fn refs(&self) -> GitTask<Vec<Ref>> {
            self.refs.set(self.refs.get() + 1);
            async { Ok(vec![]) }.boxed()
        }

        fn merge_branch(&self, _: BranchRef, _: NonEmpty) -> GitTask<MergeOutcome> {
            let outcome = self.merged.get().unwrap();
            async move { Ok(outcome) }.boxed()
        }

        fn delete_branch(&self, _: BranchRef) -> GitTask<()> {
            let deleted = self.deleting.borrow_mut().take().unwrap();
            async move {
                let _ = deleted.await;
                Ok(())
            }
            .boxed()
        }
    }

    /// The repository, showing its branches.
    fn open(git: &Rc<Git>, cx: &mut TestAppContext) -> Entity<Repository> {
        let shared: Rc<dyn RepositoryGit> = git.clone();
        let repository = cx.new(|_| Repository::new(repository_id(), shared));
        repository.update(cx, |repository, cx| repository.show(Field::Refs, cx));
        cx.run_until_parked();
        assert_eq!(git.refs.get(), 1);
        repository
    }

    fn merge_feature() -> Merge {
        Merge {
            reference: BranchRef::parse("refs/heads/feature").unwrap(),
            into: NonEmpty::parse("main").unwrap(),
        }
    }

    // `git/queries/branch.test.ts`.

    #[gpui_kit::test]
    fn fails_a_merge_that_changed_nothing_without_reloading(cx: &mut TestAppContext) {
        let git = Rc::new(Git::default());
        git.merged.set(Some(MergeOutcome::UpToDate));
        let repository = open(&git, cx);
        let done = repository
            .update(cx, |repository, cx| repository.merge(merge_feature(), cx))
            .unwrap();
        cx.run_until_parked();
        let error = done.now_or_never().unwrap().unwrap().unwrap_err();
        assert_eq!(
            error.to_string(),
            "Already up to date: main has every commit of feature."
        );
        assert_eq!(git.refs.get(), 1);
    }

    #[gpui_kit::test]
    fn reloads_after_a_merge_that_stopped_at_conflicts(cx: &mut TestAppContext) {
        let git = Rc::new(Git::default());
        git.merged.set(Some(MergeOutcome::Conflicts));
        let repository = open(&git, cx);
        let done = repository
            .update(cx, |repository, cx| repository.merge(merge_feature(), cx))
            .unwrap();
        cx.run_until_parked();
        assert_eq!(
            done.now_or_never().unwrap().unwrap().unwrap(),
            MergeOutcome::Conflicts
        );
        assert_eq!(git.refs.get(), 2);
    }

    #[gpui_kit::test]
    fn switching_to_the_checked_out_branch_does_nothing(cx: &mut TestAppContext) {
        let git = Rc::new(Git::default());
        let repository = open(&git, cx);
        let status = Uncommitted {
            head: Head::Branch {
                name: "main".into(),
                sha: "abc".into(),
            },
            upstream: None,
            ahead: 0,
            behind: 0,
            counts: StatusCounts {
                files: 0,
                staged: 0,
                unstaged: 0,
                conflicted: 0,
            },
            changes: WorkingTreeFiles {
                staged: vec![],
                unstaged: vec![],
                uncounted: false,
                marker_free: vec![],
            },
            version: "v1".into(),
        };
        // Switching would fail: this git doesn't.
        let done = repository
            .update(cx, |repository, cx| {
                repository.set_status(status, cx);
                repository.switch_branch(BranchRef::parse("refs/heads/main").unwrap(), cx)
            })
            .unwrap();
        cx.run_until_parked();
        assert!(done.now_or_never().unwrap().unwrap().is_ok());
        assert_eq!(git.refs.get(), 1);
    }

    #[gpui_kit::test]
    fn changes_one_branch_at_a_time(cx: &mut TestAppContext) {
        let git = Rc::new(Git::default());
        git.merged.set(Some(MergeOutcome::Merged));
        let (finish, finished) = oneshot::channel();
        *git.deleting.borrow_mut() = Some(finished);
        let repository = open(&git, cx);
        let _deleting = repository
            .update(cx, |repository, cx| {
                repository.delete_branch(BranchRef::parse("refs/heads/old").unwrap(), cx)
            })
            .unwrap();
        assert!(repository.read_with(cx, |repository, _| repository.branches_changing()));
        let merging = repository.update(cx, |repository, cx| repository.merge(merge_feature(), cx));
        assert!(merging.is_none());
        finish.send(()).unwrap();
        cx.run_until_parked();
        assert!(!repository.read_with(cx, |repository, _| repository.branches_changing()));
    }

    #[gpui_kit::test]
    fn shows_input_a_view_couldnt_make_as_its_change_failing(cx: &mut TestAppContext) {
        let git = Rc::new(Git::default());
        let repository = open(&git, cx);
        let done = repository.update(cx, |repository, cx| {
            let reference = BranchRef::parse("");
            repository.parsing(Change::Switch, reference, Repository::switch_branch, cx)
        });
        // It didn't run, and says why.
        assert!(done.is_none());
        let state =
            repository.read_with(cx, |repository, _| repository.change_state(Change::Switch));
        assert!(!state.running);
        assert!(state.error.is_some());
    }

    #[test]
    fn groups_the_changes_that_run_one_at_a_time() {
        assert!(Change::AddWorktree.is_in(ChangeGroup::Branches));
        assert!(Change::AddWorktree.is_in(ChangeGroup::Worktrees));
        assert!(Change::Pop(PopFrom::Picked).is_in(ChangeGroup::Stashes));
        assert!(!Change::Pull.is_in(ChangeGroup::Branches));
        // The toolbar's pop and a list's are told apart.
        assert_ne!(Change::Pop(PopFrom::Toolbar), Change::Pop(PopFrom::Picked));
    }
}
