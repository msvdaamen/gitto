//! For the details' tests: made-up git, answering what a test says (TS's tests mocked `rpc`), and
//! a window to render a part of the details in.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::BoxFuture;
use std::sync::Arc;

use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::features::history::schema::Commit;
use gitto_git::features::operation::schema::Operation;
use gitto_git::features::stash::schema::Stash;
use gitto_git::features::status::schema::{
    Head, StatusCounts, StatusReply, StatusSummary, Uncommitted, WorkingTreeFiles,
};
use gitto_git::input::RepositoryId;
use gitto_git::schema::FileStatus;
use gpui_kit::prelude::*;
use gpui_kit::test::{TestEventExt as _, TestQueryExt as _, TestWindowExt as _};
use gpui_kit::{
    AnyWindowHandle, App, Context, Entity, MouseButton, Render, Task, TestAppContext, Window, div,
};

use super::details_data::DetailsData;
use crate::data::Opaque;
use crate::data::commit::NewCommit;
use crate::data::diff::{ChangedFiles, FilesOf};
use crate::data::staging::{DiscardTarget, StagingTarget};
use crate::data::status::GetStatus;
use crate::data::{Mutation, QueryError, QueryOptions, StaleTime, keys};

pub fn repository_id() -> RepositoryId {
    RepositoryId::parse("01890a5d-ac96-774b-bcce-b302099a8057").unwrap()
}

/// Another repository: another worktree, say.
pub fn other_repository_id() -> RepositoryId {
    RepositoryId::parse("01890a5d-ac96-774b-bcce-b302099a8058").unwrap()
}

/// How a call is answered: at once, or when the test sends the answer.
enum Answer<T> {
    Now(Result<T, QueryError>),
    Later(oneshot::Receiver<Result<T, QueryError>>),
}

/// What a call is answered when the test lined nothing up, by what it was asked with.
type DefaultAnswer<T> = Box<dyn Fn(&str) -> Result<T, QueryError>>;

/// The answers to one kind of call: the ones the test lined up, in turn, then the default's. Keeps
/// what each was asked with.
pub struct Answers<T> {
    queue: RefCell<VecDeque<Answer<T>>>,
    default: DefaultAnswer<T>,
    calls: RefCell<Vec<String>>,
}

impl<T: Send + 'static> Answers<T> {
    pub fn new(default: impl Fn(&str) -> Result<T, QueryError> + 'static) -> Self {
        Answers {
            queue: RefCell::new(VecDeque::new()),
            default: Box::new(default),
            calls: RefCell::new(Vec::new()),
        }
    }

    /// The next call is answered `result`.
    pub fn once(&self, result: Result<T, QueryError>) {
        self.queue.borrow_mut().push_back(Answer::Now(result));
    }

    /// The next call is answered with what's sent on the sender returned.
    pub fn later(&self) -> oneshot::Sender<Result<T, QueryError>> {
        let (send, receive) = oneshot::channel();
        self.queue.borrow_mut().push_back(Answer::Later(receive));
        send
    }

    /// What it's been asked with, in turn.
    pub fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }

    fn answer(&self, asked: &str) -> BoxFuture<'static, Result<T, QueryError>> {
        self.calls.borrow_mut().push(asked.to_owned());
        match self.queue.borrow_mut().pop_front() {
            Some(Answer::Now(result)) => async move { result }.boxed(),
            Some(Answer::Later(answer)) => async move {
                answer
                    .await
                    .unwrap_or_else(|_| Err(QueryError::msg("Never answered.")))
            }
            .boxed(),
            None => {
                let result = (self.default)(asked);
                async move { result }.boxed()
            }
        }
    }

    /// Runs a mutation's call, as a task.
    fn run(&self, asked: &str, cx: &mut App) -> Task<Result<T, QueryError>> {
        let answer = self.answer(asked);
        cx.background_spawn(answer)
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

    /// The details' data, from these.
    pub fn data(self: &Rc<Self>) -> DetailsData {
        let (commits_of, files_of, stashes) = (Rc::clone(self), Rc::clone(self), Rc::clone(self));
        let messages = Rc::clone(self);
        let pushed_to = Rc::clone(self);
        let commits = Rc::clone(self);
        let status = Rc::clone(self);
        let operation = Rc::clone(self);
        let (stage, unstage, discard) = (Rc::clone(self), Rc::clone(self), Rc::clone(self));
        let get_status: GetStatus = Rc::new(move |_, _| {
            let gate = status.status_gate.borrow_mut().take();
            let status = status.status.borrow().clone();
            async move {
                // Answered once the test lets it through.
                if let Some(gate) = gate {
                    let _ = gate.await;
                }
                Ok(StatusReply::Status(Box::new(status)))
            }
            .boxed()
        });
        DetailsData::git()
            .with_commit(move |id, sha, _| {
                let fakes = Rc::clone(&commits_of);
                let sha = sha.to_owned();
                QueryOptions::new(keys::commit(id, &sha), move |_, _| {
                    fakes.commit.answer(&sha)
                })
                .stale_time(StaleTime::Infinite)
                .keep_previous_data()
            })
            .with_changed_files(move |id, sha, of, _| {
                let fakes = Rc::clone(&files_of);
                let sha = sha.to_owned();
                let key = match of {
                    FilesOf::Commit => keys::commit_files(id, &sha),
                    FilesOf::Stash => keys::stash_files(id, &sha),
                };
                QueryOptions::new(key, move |_, _| {
                    let sha = sha.clone();
                    fakes
                        .files
                        .answer(&sha)
                        .map(move |files| {
                            files.map(|files| ChangedFiles {
                                sha,
                                files: Opaque::new(files),
                            })
                        })
                        .boxed()
                })
                .stale_time(StaleTime::Infinite)
                .keep_previous_data()
            })
            .with_stashes(move |id, _| {
                let fakes = Rc::clone(&stashes);
                QueryOptions::new(keys::stashes(id), move |_, _| {
                    let stashes: Arc<[Stash]> = fakes.stashes.borrow().clone().into();
                    async move { Ok(stashes) }.boxed()
                })
            })
            .with_commit_message(move |id, sha, _| {
                let fakes = Rc::clone(&messages);
                let sha = sha.to_owned();
                QueryOptions::new(keys::commit_message(id, &sha), move |_, _| {
                    fakes.messages.answer(&sha)
                })
                .stale_time(StaleTime::Infinite)
            })
            .with_pushed_to(move |id, sha, _| {
                let fakes = Rc::clone(&pushed_to);
                let sha = sha.to_owned();
                QueryOptions::new(keys::pushed_to(id, &sha), move |_, _| {
                    fakes.pushed_to.answer(&sha)
                })
                .stale_time(StaleTime::Infinite)
            })
            .with_create_commit(move |cx| {
                let fakes = Rc::clone(&commits);
                Mutation::new(
                    move |(_, commit): (RepositoryId, NewCommit), cx: &mut App| {
                        fakes.committed.borrow_mut().push(commit.clone());
                        fakes.commits.run(&commit.message, cx)
                    },
                    cx,
                )
            })
            .with_status(get_status)
            .with_operation(move |id, _| {
                let fakes = Rc::clone(&operation);
                QueryOptions::new(keys::operation(id), move |_, _| {
                    let operation = fakes.operation.borrow().clone();
                    async move { Ok(operation) }.boxed()
                })
            })
            .with_staging(
                move |cx| {
                    let fakes = Rc::clone(&stage);
                    Mutation::new(
                        move |(_, target): (RepositoryId, StagingTarget), cx: &mut App| {
                            fakes.staged.run(&format!("{target:?}"), cx)
                        },
                        cx,
                    )
                },
                move |cx| {
                    let fakes = Rc::clone(&unstage);
                    Mutation::new(
                        move |(_, target): (RepositoryId, StagingTarget), cx: &mut App| {
                            fakes.unstaged.run(&format!("{target:?}"), cx)
                        },
                        cx,
                    )
                },
                move |cx| {
                    let fakes = Rc::clone(&discard);
                    Mutation::new(
                        move |(_, target): (RepositoryId, DiscardTarget), cx: &mut App| {
                            fakes.discarded.borrow_mut().push(target.clone());
                            fakes.discards.run(&format!("{target:?}"), cx)
                        },
                        cx,
                    )
                },
            )
    }

    /// The uncommitted changes are `unstaged` and `staged` from now on (TS's `setChanges`).
    pub fn set_changes(&self, unstaged: Vec<ChangedFile>, staged: Vec<ChangedFile>) {
        *self.status.borrow_mut() = uncommitted(unstaged, staged);
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

/// The uncommitted changes `unstaged` and `staged`, on main at `a1`.
pub fn uncommitted(unstaged: Vec<ChangedFile>, staged: Vec<ChangedFile>) -> Uncommitted {
    let conflicted = unstaged
        .iter()
        .filter(|change| change.status == FileStatus::Conflicted)
        .count() as u64;
    Uncommitted {
        summary: StatusSummary {
            head: Head::Branch {
                name: "main".into(),
                sha: "a1".into(),
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
    let dir = crate::test_support::init(cx, |services| services);
    // Dialogs and menus come in without animating, which runs on the wall clock: a click on one
    // while it does can miss.
    cx.update(|cx| cx.set_reduce_motion(true));
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
