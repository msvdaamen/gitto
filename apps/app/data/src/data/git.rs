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

/// Writes [`RepositoryGit`], with a method for each of `GitRepo`'s, as gitto-git lists them
/// (`for_each_repo_method!`, whose bodies are gitto-git's own): which [`GitRepository`] runs, and
/// which fails by default, for tests.
macro_rules! repository_git {
    ($(
        $(#[$attr:meta])*
        fn $name:ident($repo:ident $(, $arg:ident: $ty:ty)* $(,)?) -> $out:ty $body:block
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

gitto_git::for_each_repo_method!(repository_git);

/// A test's git that does nothing: every method fails.
pub struct NoGit;

impl RepositoryGit for NoGit {}
