//! [`GitRepo`]: a repository the UI has opened, with a method for each thing it does with it.

use std::future::Future;
use std::time::Instant;

use super::{ApiError, GitApi};
use crate::core::errors::Result;
use crate::core::repo::Repo;
use crate::core::trace::{trace, tracing};
use crate::features::branch::commands::{create_branch, switch_branch};
use crate::features::branch::delete::delete_branch;
use crate::features::branch::merge::merge_branch;
use crate::features::branch::rebase::rebase_branch;
use crate::features::branch::schema::{MergeOutcome, RebaseOutcome};
use crate::features::commit::commands::{
    create_commit, get_commit_message, get_pushed_to, reword_commit,
};
use crate::features::conflicts::commands::{ShownConflict, get_conflict, keep_side, mark_resolved};
use crate::features::conflicts::schema::{Conflict, Side};
use crate::features::diff::commands::{
    get_blob, get_commit_file_patch, get_commit_files, get_staged_file_patch,
    get_unstaged_file_patch,
};
use crate::features::diff::schema::ChangedFile;
use crate::features::diff::working_tree::{
    SaveExpectation, WorkingTreeFile, read_working_tree_file, save_working_tree_file,
};
use crate::features::history::commands::{Page, get_commit, get_log};
use crate::features::history::schema::Commit;
use crate::features::operation::commands::{abort_operation, continue_operation, get_operation};
use crate::features::operation::schema::{Operation, OperationKind};
use crate::features::overview::commands::get_overview;
use crate::features::overview::schema::Overview;
use crate::features::refs::commands::list_refs;
use crate::features::refs::schema::Ref;
use crate::features::remote::commands::{fetch_all, list_remotes};
use crate::features::remote::push::{get_push_target, push};
use crate::features::remote::schema::PushTarget;
use crate::features::staging::commands::{
    stage, stage_all, stage_lines, unstage, unstage_all, unstage_lines,
};
use crate::features::staging::discard::{Discarded, discard, discard_all};
use crate::features::staging::schema::LineSelection;
use crate::features::stash::commands::{
    drop_stash, get_stash_file_patch, get_stash_files, list_stashes, pop_stash, push_stash,
};
use crate::features::stash::schema::Stash;
use crate::features::status::commands::get_status_since;
use crate::features::status::schema::StatusReply;
use crate::features::worktree::commands::{add_worktree, list_worktrees, remove_worktree};
use crate::features::worktree::schema::Worktree;
use crate::input::{
    BranchName, BranchRef, FileInput, Folder, FullSha, Message, NonEmpty, Paths, Sha,
};

/// A repository the UI opened with [`GitApi::repo`]: each method runs a feature with it, off the
/// caller (its future is `Send + 'static`, to spawn on a background executor), and fails as the UI
/// shows it, with an [`ApiError`] that's logged under the method's name and reported if it's a
/// bug. Cheap to clone. Dropping a method's future cancels it, which isn't a failure.
#[derive(Clone)]
pub struct GitRepo {
    repo: Repo,
    api: GitApi,
}

impl GitRepo {
    pub(super) fn new(repo: Repo, api: GitApi) -> Self {
        GitRepo { repo, api }
    }

    /// The repository itself, for what has no method here (tests, say).
    pub fn repo(&self) -> &Repo {
        &self.repo
    }

    /// `input` as parsed into the type a method takes (`BranchName::parse(name)`, say), failing as
    /// the procedure's contract did when it didn't check: with `Error::InvalidInput`, which reaches
    /// the UI as "Input validation failed", as a request zod refused did, and is reported as a bug.
    /// The UI hands over what git gave it (a SHA, a ref), so it only fails on one.
    pub fn input<T>(&self, input: Result<T>) -> Result<T, ApiError> {
        input.map_err(|error| self.api.fail("input", error))
    }

    /// `task`, its failure logged and reported as `name`'s, and how long it took logged when
    /// tracing (`GITTO_TRACE=1`).
    pub(super) fn run<T, F>(
        &self,
        name: &'static str,
        task: F,
    ) -> impl Future<Output = Result<T, ApiError>> + Send + 'static + use<T, F>
    where
        F: Future<Output = Result<T>> + Send + 'static,
    {
        let api = self.api.clone();
        async move {
            let start = Instant::now();
            let result = task.await;
            if tracing() {
                let ms = start.elapsed().as_millis();
                trace(format_args!("{ms:>5}ms {name}"));
            }
            result.map_err(|error| api.fail(name, error))
        }
    }
}

/// Writes a method of [`GitRepo`] for each `fn name(repo, args…) -> T { body }`: `body` runs with
/// `repo` the `&Repo`, and the method is named, and logs its failures, as `name`.
macro_rules! methods {
    ($(
        $(#[$attr:meta])*
        fn $name:ident($repo:ident $(, $arg:ident: $ty:ty)* $(,)?) -> $out:ty $body:block
    )*) => {
        impl GitRepo {
            $(
                $(#[$attr])*
                pub fn $name(
                    &self $(, $arg: $ty)*
                ) -> impl Future<Output = Result<$out, ApiError>> + Send + 'static + use<> {
                    let repo = self.repo.clone();
                    self.run(stringify!($name), async move {
                        let $repo = &repo;
                        $body
                    })
                }
            )*
        }
    };
}

methods! {
    // Status and what's in the working tree.

    /// The uncommitted changes, or that they're unchanged since the version `since`.
    fn status(repo, since: Option<String>) -> StatusReply {
        get_status_since(repo, since.as_deref()).await
    }

    /// The file at `path` in the working tree, as the editor shows it.
    fn working_tree_file(repo, path: String) -> WorkingTreeFile {
        read_working_tree_file(repo, &path).await
    }

    /// Saves `contents` to the file at `path`, if it's still as `expected` says; its new version.
    fn save_working_tree_file(
        repo,
        path: String,
        contents: String,
        expected: SaveExpectation,
    ) -> String {
        save_working_tree_file(repo, &path, &contents, &expected).await
    }

    // Staging.

    fn stage(repo, paths: Paths) -> () {
        stage(repo, &paths).await
    }

    fn stage_all(repo) -> () {
        stage_all(repo).await
    }

    fn unstage(repo, paths: Paths) -> () {
        unstage(repo, &paths).await
    }

    fn unstage_all(repo) -> () {
        unstage_all(repo).await
    }

    /// Stages the lines `selection` picks in `file`'s version `version`; the file's new version.
    fn stage_lines(
        repo,
        file: FileInput,
        untracked: bool,
        version: String,
        selection: LineSelection,
    ) -> String {
        stage_lines(repo, &file, untracked, &version, &selection).await
    }

    /// Unstages the lines `selection` picks in `file`'s version `version`; its new version.
    fn unstage_lines(repo, file: FileInput, version: String, selection: LineSelection) -> String {
        unstage_lines(repo, &file, &version, &selection).await
    }

    fn discard(repo, file: Discarded) -> () {
        discard(repo, &file).await
    }

    fn discard_all(repo) -> () {
        discard_all(repo).await
    }

    // Diffs.

    fn staged_file_patch(repo, file: FileInput) -> String {
        get_staged_file_patch(repo, &file).await
    }

    fn unstaged_file_patch(repo, file: FileInput, untracked: bool) -> String {
        get_unstaged_file_patch(repo, &file, untracked).await
    }

    fn commit_files(repo, sha: Sha) -> Vec<ChangedFile> {
        get_commit_files(repo, &sha).await
    }

    fn commit_file_patch(repo, sha: Sha, file: FileInput) -> String {
        get_commit_file_patch(repo, &sha, &file).await
    }

    fn blob(repo, oid: FullSha) -> String {
        get_blob(repo, &oid).await
    }

    // History and commits.

    fn log(repo, page: Page) -> Vec<Commit> {
        get_log(repo, page).await
    }

    fn commit(repo, sha: Sha) -> Commit {
        get_commit(repo, &sha).await
    }

    fn commit_message(repo, sha: Sha) -> String {
        get_commit_message(repo, &sha).await
    }

    /// The remote branch the commit `sha` has been pushed to, if any.
    fn pushed_to(repo, sha: Sha) -> Option<String> {
        get_pushed_to(repo, &sha).await
    }

    /// Commits what's staged with `message`, or amends the commit `amend` with it.
    fn create_commit(repo, message: Message, amend: Option<FullSha>) -> () {
        create_commit(repo, &message, amend.as_ref()).await
    }

    /// Gives the commit `sha` the message `message`; its new SHA.
    fn reword_commit(repo, sha: FullSha, message: Message) -> String {
        reword_commit(repo, &sha, &message).await
    }

    fn overview(repo) -> Overview {
        get_overview(repo).await
    }

    // Branches and refs.

    fn refs(repo) -> Vec<Ref> {
        list_refs(repo).await
    }

    /// Creates the branch `name` from `from`, or from `HEAD`, and switches to it.
    fn create_branch(repo, name: BranchName, from: Option<BranchRef>) -> () {
        create_branch(repo, &name, from.as_ref()).await
    }

    fn switch_branch(repo, reference: BranchRef) -> () {
        switch_branch(repo, &reference).await
    }

    fn delete_branch(repo, reference: BranchRef) -> () {
        delete_branch(repo, &reference).await
    }

    /// Merges `reference` into the branch `into`.
    fn merge_branch(repo, reference: BranchRef, into: NonEmpty) -> MergeOutcome {
        merge_branch(repo, &reference, &into).await
    }

    /// Rebases the branch `branch` onto `reference`.
    fn rebase_branch(repo, reference: BranchRef, branch: NonEmpty) -> RebaseOutcome {
        rebase_branch(repo, &reference, &branch).await
    }

    // Remotes.

    fn remotes(repo) -> Vec<String> {
        list_remotes(&repo.git()).await
    }

    fn fetch(repo) -> () {
        fetch_all(repo).await
    }

    /// Pulls the checked-out branch's upstream, or, with `branch`, that local branch's (see
    /// [`super::pull`]).
    fn pull(repo, branch: Option<NonEmpty>) -> () {
        super::pull(repo, branch.as_ref()).await
    }

    fn push_target(repo) -> PushTarget {
        get_push_target(repo).await
    }

    fn push(repo, force: bool) -> () {
        push(repo, force).await
    }

    // A merge or rebase under way, and its conflicts.

    fn operation(repo) -> Option<Operation> {
        get_operation(repo).await
    }

    fn continue_operation(repo, kind: OperationKind) -> () {
        continue_operation(repo, kind).await
    }

    fn abort_operation(repo, kind: OperationKind) -> () {
        abort_operation(repo, kind).await
    }

    fn conflict(repo, path: NonEmpty) -> Conflict {
        get_conflict(repo, &path).await
    }

    /// Resolves the conflict at `path` by keeping `side`, as `shown` showed it.
    fn keep_side(repo, path: NonEmpty, side: Side, shown: ShownConflict) -> () {
        keep_side(repo, &path, side, &shown).await
    }

    /// Marks the conflict at `path` resolved, if the file's still at `version`.
    fn mark_resolved(repo, path: NonEmpty, version: Option<String>, with_markers: bool) -> () {
        mark_resolved(repo, &path, version.as_deref(), with_markers).await
    }

    // Stashes.

    fn stashes(repo) -> Vec<Stash> {
        list_stashes(repo).await
    }

    fn stash_files(repo, sha: FullSha) -> Vec<ChangedFile> {
        get_stash_files(repo, &sha).await
    }

    fn stash_file_patch(repo, sha: FullSha, file: FileInput) -> String {
        get_stash_file_patch(repo, &sha, &file).await
    }

    fn push_stash(repo) -> () {
        push_stash(repo).await
    }

    fn pop_stash(repo, sha: FullSha) -> () {
        pop_stash(repo, &sha).await
    }

    fn drop_stash(repo, sha: FullSha) -> () {
        drop_stash(repo, &sha).await
    }

    // Worktrees.

    fn worktrees(repo) -> Vec<Worktree> {
        list_worktrees(repo).await
    }

    /// Checks `branch` out in a new worktree at `folder`, as the new branch `new_branch` if given.
    fn add_worktree(
        repo,
        folder: Folder,
        branch: BranchRef,
        new_branch: Option<BranchName>,
    ) -> () {
        add_worktree(repo, &folder, &branch, new_branch.as_ref()).await
    }

    fn remove_worktree(repo, path: String) -> () {
        remove_worktree(repo, &path).await
    }
}
