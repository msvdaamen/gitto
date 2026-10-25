//! What a [`Repository`](super::repository::Repository) asks git: [`RepositoryGit`], a method
//! for each thing, which the app answers with gitto-git's [`GitRepo`](gitto_git::GitRepo) and a
//! test with what it makes up. GPUI tests can't run git (see `crate::data`), so this is where they
//! step in: a test's implements what its view asks for, and the rest fail.

use futures::FutureExt as _;
use futures::future::BoxFuture;
use gitto_git::features::branch::schema::{MergeOutcome, RebaseOutcome};
use gitto_git::features::conflicts::commands::ShownConflict;
use gitto_git::features::conflicts::schema::{Conflict, Side};
use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::features::diff::working_tree::{SaveExpectation, WorkingTreeFile};
use gitto_git::features::history::commands::Page;
use gitto_git::features::history::schema::Commit;
use gitto_git::features::operation::schema::{Operation, OperationKind};
use gitto_git::features::overview::schema::Overview;
use gitto_git::features::refs::schema::Ref;
use gitto_git::features::remote::schema::PushTarget;
use gitto_git::features::staging::discard::Discarded;
use gitto_git::features::staging::schema::LineSelection;
use gitto_git::features::stash::schema::Stash;
use gitto_git::features::status::schema::StatusReply;
use gitto_git::features::worktree::schema::Worktree;
use gitto_git::input::{
    BranchName, BranchRef, FileInput, Folder, FullSha, Message, NonEmpty, Paths, RepositoryId, Sha,
};
use gitto_git::{ApiError, GitApi};

use super::error::DataError;

/// What git does, and answers, as a future that runs on the background executor.
pub type GitTask<T> = BoxFuture<'static, Result<T, DataError>>;

/// Writes [`RepositoryGit`], with a method for each `fn`: each `GitRepo`'s method of that name,
/// which [`GitRepository`] runs, and which fails by default, for tests.
macro_rules! repository_git {
    ($(
        $(#[$attr:meta])*
        fn $name:ident($($arg:ident: $ty:ty),* $(,)?) -> $out:ty;
    )*) => {
        /// A repository's git (see the module's doc).
        pub trait RepositoryGit {
            $(
                $(#[$attr])*
                fn $name(&self $(, $arg: $ty)*) -> GitTask<$out> {
                    $(let _ = $arg;)*
                    not_in_this_test(stringify!($name))
                }
            )*

            /// `error`, from checking what the UI hands over, as the UI shows it: logged, and
            /// reported as the bug it is (see `GitRepo::input`).
            fn invalid(&self, error: gitto_git::Error) -> DataError {
                ApiError::from(error).into()
            }
        }

        impl RepositoryGit for GitRepository {
            $(
                fn $name(&self $(, $arg: $ty)*) -> GitTask<$out> {
                    let open = self.api.repo(&self.id);
                    async move { Ok(open.await?.$name($($arg),*).await?) }.boxed()
                }
            )*

            fn invalid(&self, error: gitto_git::Error) -> DataError {
                self.api.fail("input", error).into()
            }
        }
    };
}

/// What a test's [`RepositoryGit`] does with what it doesn't implement.
fn not_in_this_test<T>(name: &str) -> GitTask<T> {
    let error = DataError::msg(format!("Not in this test: {name}."));
    async move { Err(error) }.boxed()
}

/// The repository `id`, as gitto-git runs it: each method opens it (see `GitApi::repo`).
#[derive(Clone)]
pub struct GitRepository {
    api: GitApi,
    id: RepositoryId,
}

impl GitRepository {
    pub fn new(api: GitApi, id: RepositoryId) -> Self {
        GitRepository { api, id }
    }
}

repository_git! {
    fn status(since: Option<String>) -> StatusReply;
    fn working_tree_file(path: String) -> WorkingTreeFile;
    fn save_working_tree_file(
        path: String,
        contents: String,
        expected: SaveExpectation,
    ) -> String;
    fn stage(paths: Paths) -> ();
    fn stage_all() -> ();
    fn unstage(paths: Paths) -> ();
    fn unstage_all() -> ();
    fn stage_lines(
        file: FileInput,
        untracked: bool,
        version: String,
        selection: LineSelection,
    ) -> String;
    fn unstage_lines(file: FileInput, version: String, selection: LineSelection) -> String;
    fn discard(file: Discarded) -> ();
    fn discard_all() -> ();
    fn staged_file_patch(file: FileInput) -> String;
    fn unstaged_file_patch(file: FileInput, untracked: bool) -> String;
    fn commit_files(sha: Sha) -> Vec<ChangedFile>;
    fn commit_file_patch(sha: Sha, file: FileInput) -> String;
    fn blob(oid: FullSha) -> String;
    fn log(page: Page) -> Vec<Commit>;
    fn commit(sha: Sha) -> Commit;
    fn commit_message(sha: Sha) -> String;
    fn pushed_to(sha: Sha) -> Option<String>;
    fn create_commit(message: Message, amend: Option<FullSha>) -> ();
    fn reword_commit(sha: FullSha, message: Message) -> String;
    fn overview() -> Overview;
    fn refs() -> Vec<Ref>;
    fn create_branch(name: BranchName, from: Option<BranchRef>) -> ();
    fn switch_branch(reference: BranchRef) -> ();
    fn delete_branch(reference: BranchRef) -> ();
    fn merge_branch(reference: BranchRef, into: NonEmpty) -> MergeOutcome;
    fn rebase_branch(reference: BranchRef, branch: NonEmpty) -> RebaseOutcome;
    fn remotes() -> Vec<String>;
    fn fetch() -> ();
    fn pull(branch: Option<NonEmpty>) -> ();
    fn push_target() -> PushTarget;
    fn push(force: bool) -> ();
    fn operation() -> Option<Operation>;
    fn continue_operation(kind: OperationKind) -> ();
    fn abort_operation(kind: OperationKind) -> ();
    fn conflict(path: NonEmpty) -> Conflict;
    fn keep_side(path: NonEmpty, side: Side, shown: ShownConflict) -> ();
    fn mark_resolved(path: NonEmpty, version: Option<String>, with_markers: bool) -> ();
    fn stashes() -> Vec<Stash>;
    fn stash_files(sha: FullSha) -> Vec<ChangedFile>;
    fn stash_file_patch(sha: FullSha, file: FileInput) -> String;
    fn push_stash() -> ();
    fn pop_stash(sha: FullSha) -> ();
    fn drop_stash(sha: FullSha) -> ();
    fn worktrees() -> Vec<Worktree>;
    fn add_worktree(folder: Folder, branch: BranchRef, new_branch: Option<BranchName>) -> ();
    fn remove_worktree(path: String) -> ();
}

/// A test's git that does nothing: every method fails.
pub struct NoGit;

impl RepositoryGit for NoGit {}
