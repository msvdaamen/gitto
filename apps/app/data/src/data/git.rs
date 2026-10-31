//! What a [`Repository`](super::repository::Repository) asks git: [`RepositoryGit`], a method
//! for each thing, which the app answers with gitto-git's [`GitRepo`](gitto_git::GitRepo) and a
//! test with what it makes up. GPUI tests don't run git (see `crate::data`), so this is where they
//! step in: a test's implements what its view asks for, and asking it for the rest panics, so a
//! test can't pass with a part of its view failing to load unnoticed.

use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::{self, BoxFuture, Shared};
use gitto_git::Discarded;
use gitto_git::Page;
use gitto_git::ShownConflict;
use gitto_git::features::branch::schema::{MergeOutcome, RebaseOutcome};
use gitto_git::features::conflicts::schema::{Conflict, Side};
use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::features::diff::working_tree::{SaveExpectation, WorkingTreeFile};
use gitto_git::features::history::schema::Commit;
use gitto_git::features::operation::schema::{Operation, OperationKind};
use gitto_git::features::overview::schema::Overview;
use gitto_git::features::refs::schema::Ref;
use gitto_git::features::remote::schema::PushTarget;
use gitto_git::features::staging::schema::LineSelection;
use gitto_git::features::stash::schema::Stash;
use gitto_git::features::status::schema::StatusReply;
use gitto_git::features::worktree::schema::Worktree;
use gitto_git::input::{
    BranchName, BranchRef, BranchShortName, FileInput, Folder, FullSha, Message, Paths, RepoPath,
    RepositoryId, Sha,
};
use gitto_git::{ApiError, ContentVersion, GitApi, GitRepo, PatchVersion};

use super::error::DataError;

/// What git does, and answers, as a future that runs on the background executor.
pub type GitTask<T> = BoxFuture<'static, Result<T, DataError>>;

/// Writes [`RepositoryGit`], with a method for each of `GitRepo`'s, as gitto-git lists them
/// (`for_each_repo_method!`, whose bodies are gitto-git's own): which [`GitRepository`] runs, which
/// [`Removed`] fails, which [`InOrder`] puts in order, and which panics by default, for tests.
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
                    panic!("Not in this test: {}.", stringify!($name))
                }
            )*

            /// `error`, from checking what the UI hands over, as the UI shows it: logged, and
            /// reported as the bug it is (see `GitRepo::input`). A test's is only the error.
            fn invalid(&self, error: gitto_git::Error) -> DataError {
                DataError::new(error)
            }
        }

        impl RepositoryGit for GitRepository {
            $(
                fn $name(&self $(, $arg: $ty)*) -> GitTask<$out> {
                    let open = self.open();
                    let again = self.clone();
                    async move {
                        let (repo, kept) = open.await?;
                        match repo.$name($($arg),*).await {
                            Ok(out) => Ok(out),
                            Err(error) => Err(again.failed(kept, error).await),
                        }
                    }
                    .boxed()
                }
            )*

            fn invalid(&self, error: gitto_git::Error) -> DataError {
                self.api.fail("input", error).into()
            }
        }

        impl RepositoryGit for InOrder {
            $(
                fn $name(&self $(, $arg: $ty)*) -> GitTask<$out> {
                    let task = self.git.$name($($arg),*);
                    if writes(stringify!($name)) {
                        self.after_the_writes(task)
                    } else {
                        task
                    }
                }
            )*

            fn invalid(&self, error: gitto_git::Error) -> DataError {
                self.git.invalid(error)
            }
        }

        impl RepositoryGit for Removed {
            $(
                fn $name(&self $(, $arg: $ty)*) -> GitTask<$out> {
                    $(let _ = $arg;)*
                    let error = super::repository::removed();
                    async move { Err(error) }.boxed()
                }
            )*
        }
    };
}

/// Whether `GitRepo`'s method `method` changes the repository: all do but those that read it, and
/// `fetch`, which only updates what's fetched, in a line of its own (see `Repo::fetch`).
fn writes(method: &str) -> bool {
    !matches!(
        method,
        "status"
            | "working_tree_file"
            | "staged_file_patch"
            | "unstaged_file_patch"
            | "commit_files"
            | "commit_file_patch"
            | "blob"
            | "log"
            | "commit"
            | "commit_message"
            | "pushed_to"
            | "overview"
            | "refs"
            | "remotes"
            | "fetch"
            | "push_target"
            | "operation"
            | "conflict"
            | "stashes"
            | "stash_files"
            | "stash_file_patch"
            | "worktrees"
    )
}

/// A repository's git, `git`, whose writes reach git in the order they're asked for: each starts
/// once those asked for before it are done, or dropped. gitto-git's take their place in the
/// repository's line of writes only when they get to it, once the repository's open, say, not when
/// they're asked for: a push asked for after a commit could otherwise run first, and push without
/// it. What reads the repository, and fetching, doesn't wait.
pub struct InOrder {
    git: Rc<dyn RepositoryGit>,
    /// Done once the last write asked for is, and every one before it.
    last_write: RefCell<Shared<BoxFuture<'static, ()>>>,
}

impl InOrder {
    pub fn new(git: Rc<dyn RepositoryGit>) -> Self {
        InOrder {
            git,
            last_write: RefCell::new(future::ready(()).boxed().shared()),
        }
    }

    /// `write`, once the writes asked for before it are done.
    fn after_the_writes<T: Send + 'static>(&self, write: GitTask<T>) -> GitTask<T> {
        let before = self.last_write.borrow().clone();
        // `done` is dropped once `write` is done, or dropped: the writes after it wait for that, and
        // for those before it, should it be dropped before they're done.
        let (done, dropped) = oneshot::channel::<()>();
        let earlier = before.clone();
        *self.last_write.borrow_mut() = async move {
            earlier.await;
            dropped.await.ok();
        }
        .boxed()
        .shared();
        async move {
            let _done = done;
            before.await;
            write.await
        }
        .boxed()
    }
}

/// The git of a repository that was removed from Gitto: everything fails, saying so. What a view
/// that's about to go with it asks it for (see `Repositories::open`).
pub struct Removed;

/// The repository `id`, as gitto-git runs it: opened once (see `GitApi::repo`), and again after
/// a method failed, in case that's why. Cheap to clone; the clones share what's opened.
#[derive(Clone)]
pub struct GitRepository {
    api: GitApi,
    id: RepositoryId,
    opened: Arc<Mutex<Option<GitRepo>>>,
}

impl GitRepository {
    pub fn new(api: GitApi, id: RepositoryId) -> Self {
        GitRepository {
            api,
            id,
            opened: Arc::default(),
        }
    }

    /// The repository, as it was opened, or opened now: whether it was kept, with it.
    fn open(&self) -> impl Future<Output = Result<(GitRepo, bool), ApiError>> + Send + 'static {
        let kept = lock(&self.opened).clone();
        let opening = self.api.repo(&self.id);
        let opened = Arc::clone(&self.opened);
        async move {
            if let Some(repo) = kept {
                return Ok((repo, true));
            }
            let repo = opening.await?;
            *lock(&opened) = Some(repo.clone());
            Ok((repo, false))
        }
    }

    /// Why a method failed, `error`, with what was opened forgotten: the next opens it again. If
    /// it ran with what was kept, and it can't be opened now (its folder was deleted, say), that's
    /// why, as it would have said had it been opened for it: not the git that failed to start in
    /// a folder that's gone.
    async fn failed(self, kept: bool, error: ApiError) -> DataError {
        lock(&self.opened).take();
        if kept && let Err(open) = self.api.repo(&self.id).await {
            return open.into();
        }
        error.into()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

gitto_git::for_each_repo_method!(repository_git);

/// The file `file`, as git's methods take it: its path, and the one it was renamed from, checked.
pub fn file_input(file: &ChangedFile) -> gitto_git::Result<FileInput> {
    Ok(FileInput {
        path: RepoPath::parse(file.path.clone())?,
        orig_path: file.orig_path.clone().map(RepoPath::parse).transpose()?,
    })
}

/// A test's git that does nothing: asking it anything panics, for a view whose repository is given
/// everything it shows.
pub struct NoGit;

impl RepositoryGit for NoGit {}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use gitto_git::ApiCode;
    use gpui_kit::TestAppContext;

    use super::*;

    /// A repository in a folder of its own, and its git, counting how often it's looked up.
    fn repository() -> (tempfile::TempDir, GitRepository, Arc<AtomicUsize>) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("repo");
        std::fs::create_dir(&path).unwrap();
        let init = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&path)
            .output()
            .expect("git runs");
        assert!(init.status.success(), "{init:?}");
        let lookups = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&lookups);
        let api = GitApi::new(move |_: &RepositoryId| -> Option<PathBuf> {
            counted.fetch_add(1, Ordering::SeqCst);
            Some(path.clone())
        });
        let id = RepositoryId::parse("01890a5d-ac96-774b-bcce-b302099a8057").unwrap();
        (dir, GitRepository::new(api, id), lookups)
    }

    #[test]
    #[should_panic(expected = "Not in this test: refs.")]
    fn panics_when_a_test_asks_its_git_what_it_doesnt_answer() {
        drop(NoGit.refs());
    }

    /// A write asked for, by its method, and what answers it.
    type Asked = (&'static str, oneshot::Sender<Result<(), DataError>>);

    /// A repository's git that says what it got to, in turn: its writes, which answer when the test
    /// says, and its remotes, a read, which answer at once.
    #[derive(Default)]
    struct Gated {
        reached: Arc<Mutex<Vec<&'static str>>>,
        answers: RefCell<Vec<Asked>>,
    }

    impl Gated {
        fn write(&self, method: &'static str) -> GitTask<()> {
            let (answer, answered) = oneshot::channel();
            self.answers.borrow_mut().push((method, answer));
            let reached = Arc::clone(&self.reached);
            async move {
                lock(&reached).push(method);
                answered.await.unwrap()
            }
            .boxed()
        }

        /// Answers the write `method` with `answer`.
        fn answer(&self, method: &str, answer: Result<(), DataError>) {
            let mut answers = self.answers.borrow_mut();
            let at = answers.iter().position(|(asked, _)| *asked == method);
            drop(answers.remove(at.unwrap()).1.send(answer));
        }

        fn reached(&self) -> Vec<&'static str> {
            lock(&self.reached).clone()
        }
    }

    impl RepositoryGit for Gated {
        fn stage_all(&self) -> GitTask<()> {
            self.write("stage_all")
        }

        fn discard_all(&self) -> GitTask<()> {
            self.write("discard_all")
        }

        fn push(&self, _: bool) -> GitTask<()> {
            self.write("push")
        }

        fn remotes(&self) -> GitTask<Vec<String>> {
            let reached = Arc::clone(&self.reached);
            async move {
                lock(&reached).push("remotes");
                Ok(Vec::new())
            }
            .boxed()
        }
    }

    #[gpui_kit::test]
    fn runs_the_writes_in_the_order_theyre_asked_for_and_reads_meanwhile(cx: &mut TestAppContext) {
        let gated = Rc::new(Gated::default());
        let git = InOrder::new(gated.clone());
        let stage = cx.executor().spawn(git.stage_all());
        let discard = cx.executor().spawn(git.discard_all());
        let push = cx.executor().spawn(git.push(false));
        let remotes = cx.executor().spawn(git.remotes());
        cx.run_until_parked();
        let mut reached = gated.reached();
        reached.sort();
        assert_eq!(reached, ["remotes", "stage_all"]);
        assert!(smol::block_on(remotes).is_ok());

        // Once the one before is done, failed or not.
        gated.answer("stage_all", Err(DataError::msg("Couldn't stage.")));
        cx.run_until_parked();
        assert_eq!(gated.reached()[2..], ["discard_all"]);
        gated.answer("discard_all", Ok(()));
        cx.run_until_parked();
        assert_eq!(gated.reached()[2..], ["discard_all", "push"]);
        gated.answer("push", Ok(()));
        cx.run_until_parked();
        assert!(smol::block_on(stage).is_err());
        assert!(smol::block_on(discard).is_ok());
        assert!(smol::block_on(push).is_ok());
    }

    #[gpui_kit::test]
    fn holds_a_write_up_for_those_before_one_dropped_before_them(cx: &mut TestAppContext) {
        let gated = Rc::new(Gated::default());
        let git = InOrder::new(gated.clone());
        let stage = cx.executor().spawn(git.stage_all());
        drop(git.discard_all());
        let push = cx.executor().spawn(git.push(false));
        cx.run_until_parked();
        assert_eq!(gated.reached(), ["stage_all"]);

        gated.answer("stage_all", Ok(()));
        cx.run_until_parked();
        assert_eq!(gated.reached(), ["stage_all", "push"]);
        gated.answer("push", Ok(()));
        cx.run_until_parked();
        assert!(smol::block_on(stage).is_ok());
        assert!(smol::block_on(push).is_ok());
    }

    #[test]
    fn opens_the_repository_once_for_what_it_runs() {
        let (_dir, git, lookups) = repository();
        smol::block_on(git.remotes()).unwrap();
        smol::block_on(git.clone().remotes()).unwrap();
        assert_eq!(lookups.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn says_the_folder_is_gone_once_its_deleted_and_opens_it_again_after() {
        let (dir, git, lookups) = repository();
        smol::block_on(git.remotes()).unwrap();
        std::fs::remove_dir_all(dir.path().join("repo")).unwrap();

        let error = smol::block_on(git.remotes()).unwrap_err();
        assert!(error.has_code(ApiCode::NotFound), "{error}");
        assert!(error.to_string().ends_with("no longer exists."), "{error}");
        // Opened again for the next: it's not kept as it was.
        assert!(smol::block_on(git.remotes()).is_err());
        assert_eq!(lookups.load(Ordering::SeqCst), 3);
    }
}
