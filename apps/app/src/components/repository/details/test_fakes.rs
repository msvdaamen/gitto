//! For the details' tests: made-up git, answering what a test says (TS's tests mocked `rpc`), and
//! a window to render a part of the details in.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::BoxFuture;

use gitto_git::Discarded;
use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::features::history::schema::Commit;
use gitto_git::features::operation::schema::Operation;
use gitto_git::features::stash::schema::Stash;
use gitto_git::features::status::schema::{
    Head, StatusCounts, StatusReply, StatusSummary, Uncommitted, WorkingTreeFiles,
};
use gitto_git::input::{FullSha, Message, Paths, RepositoryId, Sha};
use gitto_git::schema::FileStatus;
use gpui_kit::prelude::*;
use gpui_kit::test::{TestEventExt as _, TestQueryExt as _, TestWindowExt as _};
use gpui_kit::{
    AnyWindowHandle, App, Context, Entity, MouseButton, Render, TestAppContext, Window, div,
};

use crate::components::repository::test_support::fake_git;
use gitto_app_data::data::DataError;
use gitto_app_data::data::commit::NewCommit;
use gitto_app_data::data::git::{GitTask, RepositoryGit};
use gitto_app_data::data::staging::{DiscardTarget, StagingTarget};

pub fn repository_id() -> RepositoryId {
    RepositoryId::parse("01890a5d-ac96-774b-bcce-b302099a8057").unwrap()
}

/// Another repository: another worktree, say.
pub fn other_repository_id() -> RepositoryId {
    RepositoryId::parse("01890a5d-ac96-774b-bcce-b302099a8058").unwrap()
}

/// How a call is answered: at once, or when the test sends the answer.
enum Answer<T> {
    Now(Result<T, DataError>),
    Later(oneshot::Receiver<Result<T, DataError>>),
}

/// What a call is answered when the test lined nothing up, by what it was asked with.
type DefaultAnswer<T> = Box<dyn Fn(&str) -> Result<T, DataError>>;

/// The answers to one kind of call: the ones the test lined up, in turn, then the default's. Keeps
/// what each was asked with.
pub struct Answers<T> {
    queue: RefCell<VecDeque<Answer<T>>>,
    default: DefaultAnswer<T>,
    calls: RefCell<Vec<String>>,
}

impl<T: Send + 'static> Answers<T> {
    pub fn new(default: impl Fn(&str) -> Result<T, DataError> + 'static) -> Self {
        Answers {
            queue: RefCell::new(VecDeque::new()),
            default: Box::new(default),
            calls: RefCell::new(Vec::new()),
        }
    }

    /// The next call is answered `result`.
    pub fn once(&self, result: Result<T, DataError>) {
        self.queue.borrow_mut().push_back(Answer::Now(result));
    }

    /// The next call is answered with what's sent on the sender returned.
    pub fn later(&self) -> oneshot::Sender<Result<T, DataError>> {
        let (send, receive) = oneshot::channel();
        self.queue.borrow_mut().push_back(Answer::Later(receive));
        send
    }

    /// What it's been asked with, in turn.
    pub fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }

    fn answer(&self, asked: &str) -> BoxFuture<'static, Result<T, DataError>> {
        self.calls.borrow_mut().push(asked.to_owned());
        match self.queue.borrow_mut().pop_front() {
            Some(Answer::Now(result)) => async move { result }.boxed(),
            Some(Answer::Later(answer)) => async move {
                answer
                    .await
                    .unwrap_or_else(|_| Err(DataError::msg("Never answered.")))
            }
            .boxed(),
            None => {
                let result = (self.default)(asked);
                async move { result }.boxed()
            }
        }
    }
}

/// Made-up git for the details: what each call answers, and what it was asked.
pub struct Fakes {
    pub commit: Answers<Commit>,
    pub files: Answers<Vec<ChangedFile>>,
    pub stashes: RefCell<Vec<Stash>>,
    pub messages: Answers<String>,
    pub pushed_to: Answers<Option<String>>,
    pub commits: Answers<()>,
    pub committed: RefCell<Vec<NewCommit>>,
    pub status: RefCell<Uncommitted>,
    /// Holds the next status read back until it's sent, if set.
    pub status_gate: RefCell<Option<oneshot::Receiver<()>>>,
    pub operation: RefCell<Option<Operation>>,
    pub staged: Answers<()>,
    pub unstaged: Answers<()>,
    pub discards: Answers<()>,
    pub discarded: RefCell<Vec<DiscardTarget>>,
}

impl Fakes {
    pub fn new() -> Rc<Self> {
        Rc::new(Fakes {
            commit: Answers::new(|sha| Ok(commit(sha))),
            files: Answers::new(|sha| {
                Ok(vec![file(
                    &format!("{sha}.txt"),
                    FileStatus::Modified,
                    None,
                )])
            }),
            stashes: RefCell::new(Vec::new()),
            messages: Answers::new(|sha| Ok(format!("Commit {sha}\n\nIts body"))),
            pushed_to: Answers::new(|_| Ok(None)),
            commits: Answers::new(|_| Ok(())),
            committed: RefCell::new(Vec::new()),
            status: RefCell::new(uncommitted(vec![], vec![])),
            status_gate: RefCell::new(None),
            operation: RefCell::new(None),
            staged: Answers::new(|_| Ok(())),
            unstaged: Answers::new(|_| Ok(())),
            discards: Answers::new(|_| Ok(())),
            discarded: RefCell::new(Vec::new()),
        })
    }

    /// The uncommitted changes are `unstaged` and `staged` from now on (TS's `setChanges`).
    pub fn set_changes(&self, unstaged: Vec<ChangedFile>, staged: Vec<ChangedFile>) {
        *self.status.borrow_mut() = uncommitted(unstaged, staged);
    }
}

impl RepositoryGit for Fakes {
    fn status(&self, _: Option<gitto_git::ContentVersion>) -> GitTask<StatusReply> {
        let gate = self.status_gate.borrow_mut().take();
        let status = self.status.borrow().clone();
        async move {
            // Answered once the test lets it through.
            if let Some(gate) = gate {
                let _ = gate.await;
            }
            Ok(StatusReply::Status(Box::new(status)))
        }
        .boxed()
    }

    fn commit(&self, sha: Sha) -> GitTask<Commit> {
        self.commit.answer(sha.as_str())
    }

    fn commit_files(&self, sha: Sha) -> GitTask<Vec<ChangedFile>> {
        self.files.answer(sha.as_str())
    }

    fn stash_files(&self, sha: FullSha) -> GitTask<Vec<ChangedFile>> {
        self.files.answer(sha.as_str())
    }

    fn stashes(&self) -> GitTask<Vec<Stash>> {
        let stashes = self.stashes.borrow().clone();
        async move { Ok(stashes) }.boxed()
    }

    fn commit_message(&self, sha: Sha) -> GitTask<String> {
        self.messages.answer(sha.as_str())
    }

    fn pushed_to(&self, sha: Sha) -> GitTask<Option<String>> {
        self.pushed_to.answer(sha.as_str())
    }

    fn create_commit(&self, message: Message, amend: Option<FullSha>) -> GitTask<()> {
        let commit = NewCommit { message, amend };
        let answer = self.commits.answer(&commit.message);
        self.committed.borrow_mut().push(commit);
        answer
    }

    fn operation(&self) -> GitTask<Option<Operation>> {
        let operation = self.operation.borrow().clone();
        async move { Ok(operation) }.boxed()
    }

    fn stage(&self, paths: Paths) -> GitTask<()> {
        let target = StagingTarget::Paths(
            paths
                .as_slice()
                .iter()
                .map(|path| path.as_str().to_owned())
                .collect(),
        );
        self.staged.answer(&format!("{target:?}"))
    }

    fn stage_all(&self) -> GitTask<()> {
        self.staged.answer(&format!("{:?}", StagingTarget::All))
    }

    fn unstage(&self, paths: Paths) -> GitTask<()> {
        let target = StagingTarget::Paths(
            paths
                .as_slice()
                .iter()
                .map(|path| path.as_str().to_owned())
                .collect(),
        );
        self.unstaged.answer(&format!("{target:?}"))
    }

    fn unstage_all(&self) -> GitTask<()> {
        self.unstaged.answer(&format!("{:?}", StagingTarget::All))
    }

    fn discard(&self, discarded: Discarded) -> GitTask<()> {
        let Discarded {
            path,
            orig_path,
            status,
            side,
        } = discarded;
        // What the details asked to discard: the file as their list has it.
        let file = file(&path, status, orig_path.as_deref());
        self.discard_target(DiscardTarget::File { side, file })
    }

    fn discard_all(&self) -> GitTask<()> {
        self.discard_target(DiscardTarget::All)
    }
}

impl Fakes {
    fn discard_target(&self, target: DiscardTarget) -> GitTask<()> {
        let answer = self.discards.answer(&format!("{target:?}"));
        self.discarded.borrow_mut().push(target);
        answer
    }
}

/// A commit by Ada, `sha`, whose subject is "Commit `sha`".
pub fn commit(sha: &str) -> Commit {
    Commit {
        sha: sha.into(),
        parents: vec![],
        author_name: "Ada Lovelace".into(),
        author_email: "ada@example.com".into(),
        authored_at: 0,
        committed_at: 0,
        refs: vec![],
        subject: format!("Commit {sha}"),
        body: String::new(),
    }
}

/// A file with a line added, at `path`.
pub fn file(path: &str, status: FileStatus, orig_path: Option<&str>) -> ChangedFile {
    ChangedFile {
        path: path.into(),
        status,
        orig_path: orig_path.map(Into::into),
        additions: Some(1),
        deletions: Some(0),
    }
}

/// The uncommitted changes `unstaged` and `staged`, on main at `a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1`.
pub fn uncommitted(unstaged: Vec<ChangedFile>, staged: Vec<ChangedFile>) -> Uncommitted {
    let conflicted = unstaged
        .iter()
        .filter(|change| change.status == FileStatus::Conflicted)
        .count() as u64;
    Uncommitted {
        summary: StatusSummary {
            head: Head::Branch {
                name: "main".into(),
                sha: "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1".into(),
            },
            upstream: None,
            ahead: 0,
            behind: 0,
            counts: StatusCounts {
                files: (unstaged.len() + staged.len()) as u64,
                staged: staged.len() as u64,
                unstaged: unstaged.len() as u64 - conflicted,
                conflicted,
            },
        },
        changes: WorkingTreeFiles {
            staged,
            unstaged,
            uncounted: false,
            marker_free: vec![],
        },
        version: uuid::Uuid::new_v4().to_string().into(),
    }
}

/// Shows a part of the details, the view `build` makes, filling a window.
pub struct Host<V: 'static> {
    pub view: Entity<V>,
}

impl<V: Render> Render for Host<V> {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.view.clone())
    }
}

/// Opens a window showing the view `build` makes, on the app's globals; the folder they keep
/// their files in lives as long as what's returned.
pub fn open<V: Render>(
    cx: &mut TestAppContext,
    build: impl FnOnce(&mut Window, &mut Context<V>) -> V + 'static,
) -> (tempfile::TempDir, AnyWindowHandle, Entity<V>) {
    open_on(cx, None, build)
}

/// [`open`], with `fakes` as the git of the test's repositories.
pub fn open_with<V: Render>(
    cx: &mut TestAppContext,
    fakes: &Rc<Fakes>,
    build: impl FnOnce(&mut Window, &mut Context<V>) -> V + 'static,
) -> (tempfile::TempDir, AnyWindowHandle, Entity<V>) {
    open_on(cx, Some(fakes), build)
}

fn open_on<V: Render>(
    cx: &mut TestAppContext,
    fakes: Option<&Rc<Fakes>>,
    build: impl FnOnce(&mut Window, &mut Context<V>) -> V + 'static,
) -> (tempfile::TempDir, AnyWindowHandle, Entity<V>) {
    let dir = crate::test_support::init(cx, |services| services);
    if let Some(fakes) = fakes {
        for id in [repository_id(), other_repository_id()] {
            fake_git(&id, Rc::clone(fakes) as Rc<dyn RepositoryGit>, cx);
        }
    }
    let (window, host) = crate::test_support::open(cx, move |window, cx| Host {
        view: cx.new(|cx| build(window, cx)),
    });
    let view = host.read_with(cx, |host, _| host.view.clone());
    cx.run_until_parked();
    (dir, window, view)
}

/// Clicks the element labelled `label`, with the button `button`, where it is.
pub fn click_label(window: &mut Window, label: &str, button: MouseButton, cx: &mut App) {
    window.render_frame(cx);
    let center = window.find_by_label(label).bounds().center();
    window.pointer_down(center, button, cx);
    window.pointer_up(center, button, cx);
}
