//! The history's UI tests, ported from its `*.test.tsx`: `history-keyboard`, `history-paging`,
//! `history-branches`, `commit-menu` and `history-graph`, and what of `commit-selection` is the
//! history's. Each renders the real table, with the page's part played by [`Host`]: it takes the
//! selection the table asks for. GPUI tests can't run git, so the repository's is made up
//! ([`Calls`]), and what the table shows is given its data first.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use futures::FutureExt as _;
use futures::channel::oneshot;
use gitto_git::features::branch::schema::{MergeOutcome, RebaseOutcome};
use gitto_git::features::history::schema::{Commit, CommitRef, CommitRefKind};
use gitto_git::features::operation::schema::Operation;
use gitto_git::features::refs::schema::{Ref, RefKind};
use gitto_git::features::stash::schema::Stash;
use gitto_git::features::status::schema::{Head, Uncommitted};
use gitto_git::features::worktree::schema::Worktree;
use gitto_git::input::RepositoryId;
use gitto_ui_logic::git::rows::{WIP_ID, stash_row_id};
use gpui_kit::component::WindowExt as _;
use gpui_kit::test::{TestEventExt as _, TestQueryExt as _, TestWindowExt as _};
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Context, Entity, Focusable as _, IntoElement, MouseButton,
    ParentElement as _, Render, Role, ScrollStrategy, Styled as _, Subscription, TestAppContext,
    Window, div,
};

use super::columns::{keyed, part};
use super::history_table::{HistoryEvent, HistoryTable};
use super::reword_dialog::Note;
use crate::components::repository::add_worktree_dialog::AddWorktreeDialog;
use crate::components::repository::branch_menu::BranchMenuProvider;
use crate::components::repository::create_branch_dialog::CreateBranchDialog;
use crate::components::repository::test_support::{
    Repositories, fake_git, repository_entity, repository_id, set_files, set_operation, set_refs,
    set_remotes, set_stashes, set_status, set_worktrees, setup_with, uncommitted,
};
use gitto_app_data::data::branch::{Merge, NewBranchFrom, Rebase};
use gitto_app_data::data::commit::Reword;
use gitto_app_data::data::diff::{ChangedFiles, FilesOf};
use gitto_app_data::data::git::{GitTask, RepositoryGit};
use gitto_app_data::data::{DataError, Opaque, Part};
use gitto_git::features::history::commands::Page;
use gitto_git::features::status::schema::StatusReply;
use gitto_git::input::{BranchName, BranchRef, FullSha, Message, NonEmpty};

/// A commit of the log, `sha`, with `parents`, made `committed_at`.
fn commit(sha: &str, subject: &str, parents: &[&str], refs: Vec<CommitRef>) -> Commit {
    Commit {
        sha: sha.into(),
        parents: parents.iter().map(|parent| (*parent).to_owned()).collect(),
        author_name: "Ada Lovelace".into(),
        author_email: "ada@example.com".into(),
        authored_at: 0,
        committed_at: 0,
        refs,
        subject: subject.into(),
        body: String::new(),
    }
}

/// A line of commits, newest first: each made on the next.
fn line(shas: &[&str]) -> Vec<Commit> {
    shas.iter()
        .enumerate()
        .map(|(ix, sha)| {
            let parents: Vec<&str> = shas.get(ix + 1).into_iter().copied().collect();
            let mut commit = commit(sha, &format!("Commit {sha}"), &parents, vec![]);
            commit.committed_at = 5000 - ix as i64 * 1000;
            commit
        })
        .collect()
}

fn commit_ref(kind: CommitRefKind, full_name: &str, current: bool) -> CommitRef {
    let name = match kind {
        CommitRefKind::Head => "HEAD".to_owned(),
        _ => full_name
            .splitn(3, '/')
            .nth(2)
            .unwrap_or(full_name)
            .to_owned(),
    };
    CommitRef {
        kind,
        name,
        full_name: full_name.into(),
        current: current.then_some(true),
    }
}

/// On main, at `sha`, with `staged` files staged.
fn on_main(sha: &str, staged: u64) -> Uncommitted {
    let mut status = uncommitted(Head::Branch {
        name: "main".into(),
        sha: sha.into(),
    });
    status.counts.staged = staged;
    status.counts.files = staged;
    status.version = uuid::Uuid::now_v7().to_string();
    status
}

/// How a rewording goes: the commit it makes, and the log and HEAD after it; or why it fails.
type RewordOutcome = Result<(String, Vec<Commit>, Uncommitted), DataError>;

/// The repository's git, made up: what it has (its log, which it reads pages of, its status, its
/// stashes…), and what the table and the menus asked it to do.
#[derive(Default)]
struct Calls {
    log: RefCell<Vec<Commit>>,
    /// Holds the next page of the log back until the test lets it through, if set.
    hold: RefCell<Option<oneshot::Receiver<()>>>,
    status: RefCell<Option<Uncommitted>>,
    stashes: RefCell<Vec<Stash>>,
    worktrees: RefCell<Vec<Worktree>>,
    refs: RefCell<Vec<Ref>>,
    /// How the next rewording goes.
    reword: RefCell<Option<RewordOutcome>>,
    /// The pages of the log asked for: what they skipped, and their limit.
    pages: RefCell<Vec<(u32, u32)>>,
    switched: RefCell<Vec<String>>,
    reworded: RefCell<Vec<Reword>>,
    merges: RefCell<Vec<Merge>>,
    rebases: RefCell<Vec<Rebase>>,
    created: RefCell<Vec<NewBranchFrom>>,
}

impl RepositoryGit for Calls {
    fn log(&self, page: Page) -> GitTask<Vec<Commit>> {
        let (skip, limit) = (page.skip(), page.limit());
        self.pages.borrow_mut().push((skip, limit));
        let page: Vec<Commit> = self
            .log
            .borrow()
            .iter()
            .skip(skip as usize)
            .take(limit as usize)
            .cloned()
            .collect();
        let hold = self.hold.borrow_mut().take();
        async move {
            if let Some(hold) = hold {
                let _ = hold.await;
            }
            Ok(page)
        }
        .boxed()
    }

    fn status(&self, _: Option<String>) -> GitTask<StatusReply> {
        let status = self.status.borrow().clone();
        async move {
            let status = status.ok_or_else(|| DataError::msg("No status in this test."))?;
            Ok(StatusReply::Status(Box::new(status)))
        }
        .boxed()
    }

    fn stashes(&self) -> GitTask<Vec<Stash>> {
        let stashes = self.stashes.borrow().clone();
        async move { Ok(stashes) }.boxed()
    }

    fn worktrees(&self) -> GitTask<Vec<Worktree>> {
        let worktrees = self.worktrees.borrow().clone();
        async move { Ok(worktrees) }.boxed()
    }

    fn refs(&self) -> GitTask<Vec<Ref>> {
        let refs = self.refs.borrow().clone();
        async move { Ok(refs) }.boxed()
    }

    fn remotes(&self) -> GitTask<Vec<String>> {
        async { Ok(vec!["origin".to_owned()]) }.boxed()
    }

    fn operation(&self) -> GitTask<Option<Operation>> {
        async { Ok(None) }.boxed()
    }

    fn switch_branch(&self, reference: BranchRef) -> GitTask<()> {
        self.switched
            .borrow_mut()
            .push(reference.as_str().to_owned());
        async { Ok(()) }.boxed()
    }

    fn reword_commit(&self, sha: FullSha, message: Message) -> GitTask<String> {
        self.reworded.borrow_mut().push(Reword {
            sha: sha.as_str().to_owned(),
            message: message.as_str().to_owned(),
        });
        let outcome = self
            .reword
            .borrow_mut()
            .take()
            .unwrap_or_else(|| Err(DataError::msg("Not in this test.")));
        // The repository reloads once it's reworded: to this log and HEAD.
        let outcome = outcome.map(|(sha, log, status)| {
            *self.log.borrow_mut() = log;
            *self.status.borrow_mut() = Some(status);
            sha
        });
        async move { outcome }.boxed()
    }

    fn merge_branch(&self, reference: BranchRef, into: NonEmpty) -> GitTask<MergeOutcome> {
        self.merges.borrow_mut().push(Merge {
            reference: reference.as_str().to_owned(),
            into: into.as_str().to_owned(),
        });
        async { Ok(MergeOutcome::Merged) }.boxed()
    }

    fn rebase_branch(&self, reference: BranchRef, branch: NonEmpty) -> GitTask<RebaseOutcome> {
        self.rebases.borrow_mut().push(Rebase {
            reference: reference.as_str().to_owned(),
            branch: branch.as_str().to_owned(),
        });
        async { Ok(RebaseOutcome::Rebased) }.boxed()
    }

    fn create_branch(&self, name: BranchName, from: Option<BranchRef>) -> GitTask<()> {
        self.created.borrow_mut().push(NewBranchFrom {
            name: name.as_str().to_owned(),
            from: from
                .map(|from| from.as_str().to_owned())
                .unwrap_or_default(),
        });
        async { Ok(()) }.boxed()
    }
}

/// Loads the repository `id`'s log again, as the watcher does once a branch moves.
fn reload_log(id: &RepositoryId, cx: &mut TestAppContext) {
    cx.update(|cx| {
        repository_entity(id, cx)
            .update(cx, |repository, cx| drop(repository.reload(Part::Log, cx)))
    });
}

/// The page's part: the branch menus' provider, and the table, whose selection it takes. The
/// details follow the selection with `details_follow`, and show nothing otherwise.
struct Host {
    table: Entity<HistoryTable>,
    provider: Entity<BranchMenuProvider>,
    selections: Rc<RefCell<Vec<String>>>,
    _subscription: Subscription,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().child(self.table.clone())
    }
}

/// A rendered history, and what it ran.
struct Rendered {
    _dir: tempfile::TempDir,
    id: RepositoryId,
    window: AnyWindowHandle,
    host: Entity<Host>,
    table: Entity<HistoryTable>,
    calls: Rc<Calls>,
    /// The repositories added to Gitto: a worktree opened is added.
    source: Rc<Repositories>,
}

impl Rendered {
    /// The folders of the worktrees opened.
    fn opened_worktrees(&self) -> Vec<String> {
        self.source
            .added
            .borrow()
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect()
    }
}

/// What a test starts the history with.
struct Start {
    log: Vec<Commit>,
    status: Uncommitted,
    stashes: Vec<Stash>,
    worktrees: Vec<Worktree>,
    refs: Vec<Ref>,
    selected: Option<String>,
    details_follow: bool,
}

impl Default for Start {
    fn default() -> Self {
        Start {
            log: line(&[
                "e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5",
                "d4",
                "c3",
                "b2",
                "a1",
            ]),
            status: on_main("e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5", 0),
            stashes: vec![],
            worktrees: vec![],
            refs: vec![],
            selected: None,
            details_follow: false,
        }
    }
}

fn render(cx: &mut TestAppContext, start: Start) -> Rendered {
    let source = Rc::new(Repositories::default());
    let dir = setup_with(cx, source.clone());
    let id = repository_id();
    let calls = Rc::new(Calls::default());
    *calls.log.borrow_mut() = start.log;
    *calls.status.borrow_mut() = Some(start.status.clone());
    *calls.stashes.borrow_mut() = start.stashes.clone();
    *calls.worktrees.borrow_mut() = start.worktrees.clone();
    *calls.refs.borrow_mut() = start.refs.clone();
    fake_git(&id, calls.clone(), cx);
    set_status(&id, start.status, cx);
    set_stashes(&id, start.stashes, cx);
    set_worktrees(&id, start.worktrees, cx);
    set_refs(&id, start.refs, cx);
    set_operation(&id, None, cx);
    set_remotes(&id, &["origin"], cx);
    let shown = id.clone();
    let (window, host) = crate::test_support::open(cx, move |window, cx| {
        let create = cx.new(|cx| CreateBranchDialog::new(shown.clone(), window, cx));
        let add = cx.new(|cx| AddWorktreeDialog::new(shown.clone(), window, cx));
        let provider = cx.new(|cx| BranchMenuProvider::with(shown.clone(), create, add, cx));
        let table = cx.new(|cx| HistoryTable::new(shown.clone(), window, cx));
        let selections = Rc::new(RefCell::new(Vec::new()));
        let details_follow = start.details_follow;
        let subscription = cx.subscribe(&table, {
            let selections = Rc::clone(&selections);
            move |_, table, HistoryEvent::Select(id), cx| {
                selections.borrow_mut().push(id.clone());
                let details = details_follow.then(|| id.clone());
                table.update(cx, |table, cx| {
                    table.set_selection(Some(id.clone()), details, cx)
                });
            }
        });
        if let Some(selected) = start.selected.clone() {
            table.update(cx, |table, cx| {
                table.set_selection(Some(selected), None, cx)
            });
        }
        Host {
            table,
            provider,
            selections,
            _subscription: subscription,
        }
    });
    cx.run_until_parked();
    let table = host.read_with(cx, |host, _| host.table.clone());
    Rendered {
        _dir: dir,
        id,
        window,
        host,
        table,
        calls,
        source,
    }
}

impl Rendered {
    fn selected(&self, cx: &mut TestAppContext) -> Option<String> {
        self.table.read_with(cx, |table, _| {
            table.selected().map(|row| row.id().to_owned())
        })
    }

    fn selections(&self, cx: &mut TestAppContext) -> Vec<String> {
        self.host
            .read_with(cx, |host, _| host.selections.borrow().clone())
    }

    /// Gives the list the focus, as clicking into it does.
    fn focus(&self, cx: &mut TestAppContext) {
        let table = self.table.clone();
        cx.update_window(self.window, |_, window, cx| {
            let focus = table.read(cx).focus_handle(cx);
            focus.focus(window, cx);
            window.render_frame(cx);
        })
        .unwrap();
    }

    fn is_list_focused(&self, cx: &mut TestAppContext) -> bool {
        let table = self.table.clone();
        cx.update_window(self.window, |_, window, cx| {
            table.read(cx).focus_handle(cx).is_focused(window)
        })
        .unwrap()
    }

    fn press(&self, key: &str, cx: &mut TestAppContext) {
        cx.update_window(self.window, |_, window, cx| window.press(key, cx))
            .unwrap();
        cx.run_until_parked();
    }

    fn render_frame(&self, cx: &mut TestAppContext) {
        cx.update_window(self.window, |_, window, cx| window.render_frame(cx))
            .unwrap();
        cx.run_until_parked();
    }

    /// The labels of the rows' options on show, top to bottom.
    fn options(&self, cx: &mut TestAppContext) -> Vec<(String, bool)> {
        cx.update_window(self.window, |_, window, _| {
            window
                .find_all_by_role(Role::ListBoxOption)
                .into_iter()
                .filter(|option| option.visible())
                .map(|option| {
                    (
                        option.label().unwrap_or_default().to_owned(),
                        option.selected() == Some(true),
                    )
                })
                .collect()
        })
        .unwrap()
    }

    /// Right-clicks `target`, and lists the menu that opened, if one did.
    fn menu_of(&self, target: gpui_kit::ElementId, cx: &mut TestAppContext) -> Vec<String> {
        cx.update_window(self.window, |_, window, cx| window.right_click(target, cx))
            .unwrap();
        cx.run_until_parked();
        self.menu(cx)
    }

    /// The items of the menu that's open, if one is.
    fn menu(&self, cx: &mut TestAppContext) -> Vec<String> {
        cx.update_window(self.window, |_, window, _| {
            if window.try_find("popup-menu").is_none() {
                return Vec::new();
            }
            window
                .within("popup-menu")
                .find_all_by_role(Role::MenuItem)
                .into_iter()
                .map(|item| item.label().unwrap_or_default().to_owned())
                .collect()
        })
        .unwrap()
    }

    fn choose(&self, label: &str, cx: &mut TestAppContext) {
        cx.update_window(self.window, |_, window, cx| {
            let center = window
                .within("popup-menu")
                .find_by_label(label)
                .bounds()
                .center();
            window.pointer_down(center, MouseButton::Left, cx);
            window.pointer_up(center, MouseButton::Left, cx);
        })
        .unwrap();
        cx.run_until_parked();
    }

    fn has_dialog(&self, cx: &mut TestAppContext) -> bool {
        cx.update_window(self.window, |_, window, cx| window.has_active_dialog(cx))
            .unwrap()
    }

    /// Hovers the count of the labels `sha`'s row has no room for, until they're listed.
    fn hover_more(&self, sha: &str, cx: &mut TestAppContext) {
        cx.update_window(self.window, |_, window, cx| {
            window.hover(part(&keyed("labels", sha.to_owned()), "more"), cx)
        })
        .unwrap();
        cx.executor().advance_clock(Duration::from_millis(300));
        cx.run_until_parked();
        self.render_frame(cx);
    }

    /// The id of `sha`'s row's label of the branch `full_name`.
    fn label(&self, sha: &str, full_name: &str) -> gpui_kit::ElementId {
        part(&keyed("labels", sha.to_owned()), full_name.to_owned())
    }

    fn rows(&self, cx: &mut TestAppContext) -> usize {
        self.table.read_with(cx, |table, _| table.rows().len())
    }

    fn scroll_to(&self, ix: usize, cx: &mut TestAppContext) {
        self.table.read_with(cx, |table, _| {
            table
                .scroll_handle()
                .scroll_to_item(ix, ScrollStrategy::Top)
        });
        self.render_frame(cx);
        // More of the log loads once the frame's shown the rows near its end.
        self.render_frame(cx);
    }

    fn pages(&self) -> Vec<(u32, u32)> {
        self.calls.pages.borrow().clone()
    }

    /// The row at the top of the view.
    fn top_row(&self, cx: &mut TestAppContext) -> usize {
        let table = self.table.clone();
        cx.update_window(self.window, |_, window, cx| {
            let offset = table
                .read(cx)
                .scroll_handle()
                .0
                .borrow()
                .base_handle
                .offset();
            let row = super::columns::ROW_HEIGHT.to_pixels(window.rem_size());
            (-offset.y / row).round() as usize
        })
        .unwrap()
    }
}

// --- history-keyboard.test.tsx ---------------------------------------------------------------

#[gpui_kit::test]
fn selects_the_next_and_the_previous_row_with_the_arrow_keys(cx: &mut TestAppContext) {
    let history = render(cx, Start::default());
    assert_eq!(
        history.selected(cx).as_deref(),
        Some("e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5")
    );
    history.focus(cx);
    let options = history.options(cx);
    assert_eq!(
        options[0],
        (
            "Commit e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5, by Ada Lovelace".into(),
            true
        )
    );

    history.press("down", cx);
    history.press("down", cx);
    assert_eq!(history.selected(cx).as_deref(), Some("c3"));
    let options = history.options(cx);
    assert!(options.contains(&("Commit c3, by Ada Lovelace".into(), true)));

    history.press("up", cx);
    assert_eq!(history.selected(cx).as_deref(), Some("d4"));
    // The list keeps the focus, whichever row is selected.
    assert!(history.is_list_focused(cx));
}

#[gpui_kit::test]
fn only_loads_the_files_of_the_row_the_details_show_not_of_every_row_passed(
    cx: &mut TestAppContext,
) {
    let history = render(cx, Start::default());
    for sha in ["e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5", "d4", "c3", "b2"] {
        let files = ChangedFiles {
            sha: sha.into(),
            files: Opaque::new(vec![]),
        };
        set_files(&history.id, FilesOf::Commit, files, cx);
    }
    let table = history.table.clone();
    table.update(cx, |table, cx| {
        table.set_selection(
            Some("e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5".into()),
            Some("e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5".into()),
            cx,
        )
    });
    assert_eq!(
        table.read_with(cx, |table, _| table.totals_of().map(str::to_owned)),
        Some("e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5".into())
    );

    history.focus(cx);
    history.press("down", cx);
    history.press("down", cx);
    history.press("down", cx);
    assert_eq!(history.selected(cx).as_deref(), Some("b2"));
    // The details still show e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5: no other row's files load.
    assert_eq!(
        table.read_with(cx, |table, _| table.totals_of().map(str::to_owned)),
        None
    );

    table.update(cx, |table, cx| {
        table.set_selection(Some("b2".into()), Some("b2".into()), cx)
    });
    assert_eq!(
        table.read_with(cx, |table, _| table.totals_of().map(str::to_owned)),
        Some("b2".into())
    );
}

#[gpui_kit::test]
fn stops_at_the_first_and_the_last_row(cx: &mut TestAppContext) {
    let history = render(cx, Start::default());
    history.focus(cx);

    history.press("up", cx);
    assert_eq!(
        history.selected(cx).as_deref(),
        Some("e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5")
    );
    history.press("end", cx);
    assert_eq!(history.selected(cx).as_deref(), Some("a1"));
    history.press("down", cx);
    assert_eq!(history.selected(cx).as_deref(), Some("a1"));
    history.press("home", cx);
    assert_eq!(
        history.selected(cx).as_deref(),
        Some("e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5")
    );
}

#[gpui_kit::test]
fn moves_a_page_at_a_time_with_page_down_and_page_up(cx: &mut TestAppContext) {
    let shas: Vec<String> = (0..60).map(|n| format!("c{n}")).collect();
    let refs: Vec<&str> = shas.iter().map(String::as_str).collect();
    let history = render(
        cx,
        Start {
            log: line(&refs),
            status: on_main("c0", 0),
            ..Start::default()
        },
    );
    history.focus(cx);
    // A page is what fits in view, less a row to keep one in sight.
    let table = history.table.clone();
    let (page, view, row) = cx
        .update_window(history.window, |_, window, cx| {
            let view = table
                .read(cx)
                .scroll_handle()
                .0
                .borrow()
                .last_item_size
                .unwrap()
                .item
                .height;
            let row = super::columns::ROW_HEIGHT.to_pixels(window.rem_size());
            (table.read(cx).page_size(window), view, row)
        })
        .unwrap();
    assert_eq!(page, (view / row).floor() as usize - 1);
    assert!(page > 10, "{page}");

    history.press("pagedown", cx);
    assert_eq!(history.selected(cx), Some(format!("c{page}")));
    history.press("pageup", cx);
    assert_eq!(history.selected(cx).as_deref(), Some("c0"));
}

#[gpui_kit::test]
fn starts_at_the_first_row_thats_on_show_when_the_search_hides_the_selected_one(
    cx: &mut TestAppContext,
) {
    let history = render(cx, Start::default());
    history
        .table
        .update(cx, |table, cx| table.set_search("c3".into(), cx));
    history.render_frame(cx);
    // e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5 is selected, but hidden.
    assert_eq!(
        history.options(cx),
        [("Commit c3, by Ada Lovelace".into(), false)]
    );

    history.focus(cx);
    history.press("down", cx);
    assert_eq!(history.selected(cx).as_deref(), Some("c3"));
}

#[gpui_kit::test]
fn hands_the_focus_to_the_list_when_a_row_is_clicked(cx: &mut TestAppContext) {
    let history = render(cx, Start::default());

    cx.update_window(history.window, |_, window, cx| {
        window.click(keyed("commit", "b2"), cx)
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(history.selected(cx).as_deref(), Some("b2"));
    assert!(history.is_list_focused(cx));
    history.press("down", cx);
    assert_eq!(history.selected(cx).as_deref(), Some("a1"));
}

// --- history-paging.test.tsx -----------------------------------------------------------------

/// Commit `index` of a line of `count`, newest first.
fn numbered(count: usize) -> Vec<Commit> {
    (0..count)
        .map(|index| {
            let parents: Vec<String> = (index + 1 < count)
                .then(|| format!("c{}", index + 1))
                .into_iter()
                .collect();
            Commit {
                sha: format!("c{index}"),
                parents,
                author_name: "Ada Lovelace".into(),
                author_email: "ada@example.com".into(),
                authored_at: 0,
                committed_at: (count - index) as i64 * 1000,
                refs: vec![],
                subject: format!("Commit {index}"),
                body: String::new(),
            }
        })
        .collect()
}

fn paged(count: usize, cx: &mut TestAppContext) -> Rendered {
    render(
        cx,
        Start {
            log: numbered(count),
            status: on_main("c0", 0),
            ..Start::default()
        },
    )
}

#[gpui_kit::test]
fn loads_more_as_its_end_comes_near_until_theres_none_left(cx: &mut TestAppContext) {
    let history = paged(1500, cx);
    assert_eq!(history.pages(), [(0, 200)]);

    // Well before the end of the first page.
    history.scroll_to(50, cx);
    assert_eq!(history.pages().len(), 1);

    history.scroll_to(120, cx);
    assert_eq!(history.pages(), [(0, 200), (200, 1000)]);
    assert_eq!(history.rows(cx), 1200);

    history.scroll_to(1150, cx);
    assert_eq!(history.pages().len(), 3);
    assert_eq!(history.pages()[2], (1200, 1000));
    assert_eq!(history.rows(cx), 1500);

    // The last page was short of its limit: that's the whole history.
    history.scroll_to(1490, cx);
    assert_eq!(history.pages().len(), 3);
}

#[gpui_kit::test]
fn doesnt_load_more_while_searching(cx: &mut TestAppContext) {
    let history = paged(1500, cx);
    // Only a handful of commits match, so the end is in view straight away.
    history
        .table
        .update(cx, |table, cx| table.set_search("Commit 7".into(), cx));
    history.render_frame(cx);
    history.render_frame(cx);
    assert_eq!(history.pages(), [(0, 200)]);
}

#[gpui_kit::test]
fn reloads_all_it_has_loaded_at_once_keeping_its_place(cx: &mut TestAppContext) {
    let history = paged(1500, cx);
    history.scroll_to(120, cx);
    assert_eq!(history.rows(cx), 1200);

    let id = history.id.clone();
    reload_log(&id, cx);
    cx.run_until_parked();
    assert_eq!(history.pages(), [(0, 200), (200, 1000), (0, 1200)]);
    assert_eq!(history.rows(cx), 1200);
    history.render_frame(cx);
    assert_eq!(history.top_row(cx), 120);
}

#[gpui_kit::test]
fn starts_over_from_the_first_page_when_its_reloaded_past_10000_commits(cx: &mut TestAppContext) {
    let history = paged(12_000, cx);
    let mut loaded = 200;
    while loaded <= 10_000 {
        history.scroll_to(loaded - 80, cx);
        assert_eq!(history.rows(cx), loaded + 1000);
        loaded += 1000;
    }

    history.scroll_to(0, cx);
    let id = history.id.clone();
    reload_log(&id, cx);
    cx.run_until_parked();
    assert_eq!(history.pages().last(), Some(&(0, 200)));
    assert_eq!(history.rows(cx), 200);
}

#[gpui_kit::test]
fn leaves_out_a_page_that_was_loading_as_it_was_reloaded(cx: &mut TestAppContext) {
    let history = paged(1500, cx);
    // The second page, held back until it's let through.
    let (release, held) = oneshot::channel();
    *history.calls.hold.borrow_mut() = Some(held);
    history.scroll_to(120, cx);
    assert_eq!(history.pages().len(), 2);

    history.scroll_to(0, cx);
    let id = history.id.clone();
    reload_log(&id, cx);
    cx.run_until_parked();
    // The reload cancels it, so it may not be waited for any more.
    let _ = release.send(());
    cx.run_until_parked();
    assert_eq!(history.rows(cx), 200);
}

// --- commit-menu.test.tsx --------------------------------------------------------------------

fn main_ref() -> CommitRef {
    commit_ref(CommitRefKind::Local, "refs/heads/main", true)
}

/// Two commits, e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5 on d4, with main at e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5 and checked out; e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5 selected.
fn two_commits(staged: u64) -> Start {
    Start {
        log: vec![
            commit(
                "e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5",
                "Commit e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5",
                &["d4"],
                vec![main_ref()],
            ),
            commit("d4", "Commit d4", &[], vec![]),
        ],
        status: on_main("e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5", staged),
        selected: Some("e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5".into()),
        ..Start::default()
    }
}

fn set_message(
    history: &Rendered,
    sha: &str,
    message: &str,
    pushed_to: Option<&str>,
    cx: &mut TestAppContext,
) {
    let id = history.id.clone();
    let message = message.to_owned();
    let pushed_to = pushed_to.map(str::to_owned);
    cx.update(|cx| {
        repository_entity(&id, cx).update(cx, |repository, cx| {
            repository.set_message(sha, message, cx);
            repository.set_pushed_to(sha, pushed_to, cx);
        })
    });
}

/// After rewording e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5 as f6: the log, and HEAD.
fn reworded_to_f6(staged: u64) -> (String, Vec<Commit>, Uncommitted) {
    (
        "f6".into(),
        vec![
            commit("f6", "Reworded", &["d4"], vec![main_ref()]),
            commit("d4", "Commit d4", &[], vec![]),
        ],
        on_main("f6", staged),
    )
}

impl Rendered {
    /// Right-clicks the commit `sha`, and chooses Edit message in its menu.
    fn edit_message_of(&self, sha: &str, cx: &mut TestAppContext) {
        let items = self.menu_of(keyed("commit", sha.to_owned()), cx);
        assert_eq!(items, ["Edit message…"]);
        self.choose("Edit message…", cx);
        assert!(self.has_dialog(cx));
    }

    fn dialog_fields(&self, cx: &mut TestAppContext) -> (String, String) {
        self.table.read_with(cx, |table, cx| {
            let dialog = table.commit_menu().read(cx).dialog().read(cx);
            let message = dialog.message(cx);
            (message.summary, message.description)
        })
    }

    fn notes(&self, cx: &mut TestAppContext) -> Vec<(Note, String)> {
        self.table.read_with(cx, |table, cx| {
            table.commit_menu().read(cx).dialog().read(cx).notes(cx)
        })
    }

    fn is_savable(&self, cx: &mut TestAppContext) -> bool {
        self.table.read_with(cx, |table, cx| {
            let dialog = table.commit_menu().read(cx).dialog().read(cx);
            dialog.is_savable(cx)
        })
    }

    fn type_text(&self, text: &str, cx: &mut TestAppContext) {
        cx.update_window(self.window, |_, window, cx| window.input(text, cx))
            .unwrap();
        cx.run_until_parked();
    }

    /// Whether the window shows `text`, as the label of something on show.
    fn shows(&self, text: &str, cx: &mut TestAppContext) -> bool {
        cx.update_window(self.window, |_, window, _| {
            window.try_find_by_label(text).is_some()
        })
        .unwrap()
    }
}

#[gpui_kit::test]
fn edits_the_last_commits_message_keeping_the_rewritten_commit_selected(cx: &mut TestAppContext) {
    let history = render(cx, two_commits(0));
    set_message(
        &history,
        "e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5",
        "Commit e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5\n\nWhy it was made\n",
        None,
        cx,
    );
    *history.calls.reword.borrow_mut() = Some(Ok(reworded_to_f6(0)));

    history.edit_message_of("e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5", cx);
    assert_eq!(
        history.dialog_fields(cx),
        (
            "Commit e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5".into(),
            "Why it was made".into()
        )
    );
    // Nothing to save until it's edited.
    assert!(!history.is_savable(cx));

    history.press("secondary-a", cx);
    history.press("backspace", cx);
    history.type_text("Reworded", cx);
    assert!(history.is_savable(cx));
    history.press("enter", cx);
    assert_eq!(
        *history.calls.reworded.borrow(),
        [Reword {
            sha: "e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5".into(),
            message: "Reworded\n\nWhy it was made\n".into(),
        }]
    );
    assert!(!history.has_dialog(cx));
    assert_eq!(history.selected(cx).as_deref(), Some("f6"));
}

#[gpui_kit::test]
fn keeps_the_rewritten_commit_selected_rather_than_the_uncommitted_changes(
    cx: &mut TestAppContext,
) {
    let history = render(cx, two_commits(1));
    // The history has a row for the changes, above the commit.
    assert_eq!(
        history
            .table
            .read_with(cx, |table, _| table.rows()[0].id().to_owned()),
        WIP_ID
    );
    set_message(
        &history,
        "e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5",
        "Commit e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5\n\nWhy it was made\n",
        None,
        cx,
    );
    *history.calls.reword.borrow_mut() = Some(Ok(reworded_to_f6(1)));

    history.edit_message_of("e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5", cx);
    history.type_text("!", cx);
    history.press("enter", cx);
    assert_eq!(history.selected(cx).as_deref(), Some("f6"));
    // The history fell back to the changes once the commit was gone, before it was told of f6.
    assert_eq!(history.selections(cx), [WIP_ID, "f6"]);
}

#[gpui_kit::test]
fn says_when_the_commit_is_pushed_already(cx: &mut TestAppContext) {
    let history = render(cx, two_commits(0));
    set_message(
        &history,
        "e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5",
        "Commit e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5\n",
        Some("origin/main"),
        cx,
    );

    history.edit_message_of("e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5", cx);
    assert_eq!(
        history.notes(cx),
        [(
            Note::Warning,
            "The commit is already on origin/main; editing its message means force-pushing."
                .to_owned()
        )]
    );
}

#[gpui_kit::test]
fn stays_open_with_the_edits_if_rewording_fails_saying_why(cx: &mut TestAppContext) {
    let history = render(cx, two_commits(0));
    set_message(
        &history,
        "e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5",
        "Commit e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5\n\nWhy it was made\n",
        None,
        cx,
    );
    let message = "The last commit changed before it could be amended, so nothing was amended.";
    *history.calls.reword.borrow_mut() = Some(Err(DataError::msg(message)));

    history.edit_message_of("e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5", cx);
    history.type_text("!", cx);
    history.press("enter", cx);
    assert!(history.has_dialog(cx));
    assert_eq!(history.notes(cx), [(Note::Error, message.to_owned())]);
    assert_eq!(
        history.dialog_fields(cx).0,
        "Commit e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5!"
    );
}

#[gpui_kit::test]
fn is_only_the_last_commit_of_the_checked_out_branchs(cx: &mut TestAppContext) {
    let history = render(cx, two_commits(0));
    assert!(history.menu_of(keyed("commit", "d4"), cx).is_empty());
}

#[gpui_kit::test]
fn isnt_there_with_head_detached(cx: &mut TestAppContext) {
    let history = render(
        cx,
        Start {
            status: uncommitted(Head::Detached {
                sha: "e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5".into(),
            }),
            ..two_commits(0)
        },
    );
    assert!(
        history
            .menu_of(
                keyed("commit", "e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5"),
                cx
            )
            .is_empty()
    );
}

#[gpui_kit::test]
fn cant_edit_the_message_while_an_operation_is_under_way(cx: &mut TestAppContext) {
    let history = render(cx, two_commits(0));
    set_operation(
        &history.id,
        Some(Operation::Merge {
            merging: "feature".into(),
            into: Some("main".into()),
        }),
        cx,
    );
    assert_eq!(
        history.menu_of(
            keyed("commit", "e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5"),
            cx
        ),
        ["Edit message…"]
    );
    // Disabled: choosing it opens nothing.
    history.choose("Edit message…", cx);
    assert!(!history.has_dialog(cx));
}

#[gpui_kit::test]
fn leaves_a_branchs_label_in_the_row_to_the_branchs_menu(cx: &mut TestAppContext) {
    let history = render(cx, two_commits(0));
    let items = history.menu_of(
        history.label(
            "e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5",
            "refs/heads/main",
        ),
        cx,
    );
    assert!(items.contains(&"Create branch…".to_owned()), "{items:?}");
    assert!(!items.contains(&"Edit message…".to_owned()));
}

// --- history-branches.test.tsx ---------------------------------------------------------------

fn local(name: &str, current: bool) -> Ref {
    Ref {
        name: name.into(),
        full_name: format!("refs/heads/{name}"),
        kind: RefKind::Local,
        sha: "e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5".into(),
        current,
        upstream: None,
        ahead: 0,
        behind: 0,
    }
}

fn remote(name: &str) -> Ref {
    Ref {
        name: name.into(),
        full_name: format!("refs/remotes/{name}"),
        kind: RefKind::Remote,
        sha: "e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5".into(),
        current: false,
        upstream: None,
        ahead: 0,
        behind: 0,
    }
}

fn worktree(path: &str, branch: Option<&str>, current: bool) -> Worktree {
    Worktree {
        path: path.into(),
        name: path.rsplit('/').next().unwrap().into(),
        head: Some("e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5".into()),
        branch: branch.map(Into::into),
        main: current,
        bare: false,
        current,
        locked: None,
        prunable: None,
    }
}

/// The one commit's branches and tags: `main` is checked out, the rest only show under `+3`.
fn branches(worktrees: Vec<Worktree>) -> Start {
    Start {
        log: vec![commit(
            "e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5",
            "Commit e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5",
            &[],
            vec![
                main_ref(),
                commit_ref(CommitRefKind::Local, "refs/heads/feature", false),
                commit_ref(CommitRefKind::Remote, "refs/remotes/origin/release", false),
                commit_ref(CommitRefKind::Tag, "refs/tags/v1", false),
            ],
        )],
        refs: vec![
            local("main", true),
            local("feature", false),
            remote("origin/release"),
        ],
        worktrees,
        selected: Some("e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5".into()),
        ..Start::default()
    }
}

#[gpui_kit::test]
fn switches_to_a_branch_double_clicked_among_the_rest_of_a_rows_labels(cx: &mut TestAppContext) {
    let history = render(cx, branches(vec![]));
    history.hover_more("e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5", cx);
    let release = history.label(
        "e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5",
        "refs/remotes/origin/release",
    );
    // A click alone doesn't switch.
    cx.update_window(history.window, |_, window, cx| {
        window.click(release.clone(), cx)
    })
    .unwrap();
    assert!(history.calls.switched.borrow().is_empty());
    cx.update_window(history.window, |_, window, cx| {
        window.double_click(release, cx)
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        *history.calls.switched.borrow(),
        ["refs/remotes/origin/release"]
    );
}

#[gpui_kit::test]
fn creates_a_branch_from_the_one_right_clicked_handing_the_focus_back_to_the_list(
    cx: &mut TestAppContext,
) {
    let history = render(cx, branches(vec![]));
    history.focus(cx);
    let main = history.label(
        "e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5",
        "refs/heads/main",
    );
    assert!(
        history
            .menu_of(main.clone(), cx)
            .contains(&"Create branch…".to_owned())
    );
    history.press("escape", cx);
    assert!(history.menu(cx).is_empty());
    assert!(history.is_list_focused(cx));

    history.menu_of(main, cx);
    history.choose("Create branch…", cx);
    let provider = history.host.read_with(cx, |host, _| host.provider.clone());
    let summary = provider.read_with(cx, |provider, cx| {
        provider.create_dialog().read(cx).summary(cx)
    });
    assert_eq!(summary, "From main.");
    history.type_text("topic", cx);
    history.press("enter", cx);
    assert_eq!(
        *history.calls.created.borrow(),
        [NewBranchFrom {
            name: "topic".into(),
            from: "refs/heads/main".into(),
        }]
    );
}

#[gpui_kit::test]
fn creates_a_branch_from_one_right_clicked_among_the_rest_of_a_rows_labels(
    cx: &mut TestAppContext,
) {
    let history = render(cx, branches(vec![]));
    history.hover_more("e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5", cx);
    history.menu_of(
        history.label(
            "e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5",
            "refs/heads/feature",
        ),
        cx,
    );
    history.choose("Create branch…", cx);
    history.type_text("topic", cx);
    history.press("enter", cx);
    assert_eq!(
        *history.calls.created.borrow(),
        [NewBranchFrom {
            name: "topic".into(),
            from: "refs/heads/feature".into(),
        }]
    );
}

#[gpui_kit::test]
fn merges_a_branch_right_clicked_among_the_rest_of_a_rows_labels_into_the_checked_out_one(
    cx: &mut TestAppContext,
) {
    let history = render(cx, branches(vec![]));
    history.hover_more("e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5", cx);
    let items = history.menu_of(
        history.label(
            "e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5",
            "refs/remotes/origin/release",
        ),
        cx,
    );
    assert!(
        items.contains(&"Merge origin/release into main".to_owned()),
        "{items:?}"
    );
    // The list of the rest stays open while the menu opened from it is.
    history.choose("Merge origin/release into main", cx);
    assert_eq!(
        *history.calls.merges.borrow(),
        [Merge {
            reference: "refs/remotes/origin/release".into(),
            into: "main".into(),
        }]
    );
}

#[gpui_kit::test]
fn rebases_the_checked_out_branch_onto_a_branch_right_clicked_among_a_rows_labels(
    cx: &mut TestAppContext,
) {
    let history = render(cx, branches(vec![]));
    history.hover_more("e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5", cx);
    history.menu_of(
        history.label(
            "e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5",
            "refs/remotes/origin/release",
        ),
        cx,
    );
    history.choose("Rebase main onto origin/release", cx);
    assert_eq!(
        *history.calls.rebases.borrow(),
        [Rebase {
            reference: "refs/remotes/origin/release".into(),
            branch: "main".into(),
        }]
    );
}

impl Rendered {
    /// Whether the rest of `sha`'s row's labels are listed.
    fn more_listed(&self, sha: &str, cx: &mut TestAppContext) -> bool {
        self.render_frame(cx);
        cx.update_window(self.window, |_, window, _| {
            window
                .try_find(part(&keyed("labels", sha.to_owned()), "more-labels"))
                .is_some()
        })
        .unwrap()
    }

    /// Moves the pointer onto `target`, and waits out the list's delays.
    fn hover_and_wait(&self, target: gpui_kit::ElementId, cx: &mut TestAppContext) {
        cx.update_window(self.window, |_, window, cx| window.hover(target, cx))
            .unwrap();
        cx.executor().advance_clock(Duration::from_millis(300));
        cx.run_until_parked();
    }
}

#[gpui_kit::test]
fn keeps_the_rest_of_a_rows_labels_listed_while_the_pointer_moves_from_the_count_onto_them(
    cx: &mut TestAppContext,
) {
    let history = render(cx, branches(vec![]));
    history.hover_more("e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5", cx);
    assert!(history.more_listed("e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5", cx));
    history.hover_and_wait(
        history.label(
            "e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5",
            "refs/heads/feature",
        ),
        cx,
    );
    assert!(history.more_listed("e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5", cx));
    history.hover_and_wait(
        keyed("commit", "e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5"),
        cx,
    );
    assert!(!history.more_listed("e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5", cx));
}

#[gpui_kit::test]
fn closes_the_rest_of_a_rows_labels_once_the_menu_opened_from_them_closes(cx: &mut TestAppContext) {
    let history = render(cx, branches(vec![]));
    history.hover_more("e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5", cx);
    history.menu_of(
        history.label(
            "e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5",
            "refs/heads/feature",
        ),
        cx,
    );
    // Kept while the menu is open, though the pointer's left them.
    history.hover_and_wait(
        keyed("commit", "e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5"),
        cx,
    );
    assert!(history.more_listed("e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5", cx));
    history.press("escape", cx);
    assert!(history.menu(cx).is_empty());
    cx.executor().advance_clock(Duration::from_millis(300));
    cx.run_until_parked();
    assert!(!history.more_listed("e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5", cx));
}

#[gpui_kit::test]
fn doesnt_offer_merging_the_checked_out_branchs_label_into_itself_or_rebasing_it_onto_itself(
    cx: &mut TestAppContext,
) {
    let history = render(cx, branches(vec![]));
    let items = history.menu_of(
        history.label(
            "e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5",
            "refs/heads/main",
        ),
        cx,
    );
    assert!(items.contains(&"Create branch…".to_owned()));
    assert!(!items.iter().any(|item| item.starts_with("Merge")));
    assert!(!items.iter().any(|item| item.starts_with("Rebase")));
}

#[gpui_kit::test]
fn opens_no_menu_for_a_tag(cx: &mut TestAppContext) {
    let history = render(cx, branches(vec![]));
    history.hover_more("e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5", cx);
    let tag = cx
        .update_window(history.window, |_, window, _| {
            window.find_by_label("v1").bounds().center()
        })
        .unwrap();
    cx.update_window(history.window, |_, window, cx| {
        window.pointer_down(tag, MouseButton::Right, cx);
        window.pointer_up(tag, MouseButton::Right, cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(history.menu(cx).is_empty());
}

#[gpui_kit::test]
fn closes_the_menu_once_the_repository_is_switched(cx: &mut TestAppContext) {
    let history = render(cx, branches(vec![]));
    assert!(
        !history
            .menu_of(
                history.label(
                    "e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5",
                    "refs/heads/main"
                ),
                cx
            )
            .is_empty()
    );
    let other = repository_id();
    set_status(
        &other,
        on_main("e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5", 0),
        cx,
    );
    set_operation(&other, None, cx);
    set_refs(&other, vec![], cx);
    set_worktrees(&other, vec![], cx);
    set_remotes(&other, &[], cx);
    set_stashes(&other, vec![], cx);
    let provider = history.host.read_with(cx, |host, _| host.provider.clone());
    let table = history.table.clone();
    cx.update_window(history.window, |_, window, cx| {
        provider.update(cx, |provider, cx| {
            provider.set_repository(other.clone(), window, cx)
        });
        table.update(cx, |table, cx| table.set_repository(other, window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    history.render_frame(cx);
    assert!(history.menu(cx).is_empty());
}

// The worktrees' part of `history-branches.test.tsx`.

fn with_worktrees() -> Start {
    branches(vec![
        worktree("/w/repo", Some("refs/heads/main"), true),
        worktree("/w/repo-feature", Some("refs/heads/feature"), false),
        worktree("/w/repo-check", None, false),
    ])
}

#[gpui_kit::test]
fn marks_a_branch_checked_out_in_another_worktree_and_names_one_detached_at_the_commit(
    cx: &mut TestAppContext,
) {
    let history = render(cx, with_worktrees());
    // The worktree is a label of its own, after the branches.
    let count = cx
        .update_window(history.window, |_, window, _| {
            window.try_find_by_label("+4").is_some()
        })
        .unwrap();
    assert!(count);
    history.hover_more("e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5", cx);
    assert!(history.shows("feature", cx));
    assert!(history.shows("repo-check", cx));
    let labels = history
        .table
        .read_with(cx, |table, _| match &table.rows()[0] {
            gitto_ui_logic::git::rows::HistoryRow::Commit(commit) => commit.labels.clone(),
            _ => vec![],
        });
    let titles: Vec<String> = labels.iter().map(super::ref_labels::label_title).collect();
    assert!(titles.contains(
        &"feature (local; checked out in repo-feature)\nDouble-click to open that worktree".into()
    ));
    assert!(titles.contains(&"repo-check (worktree, detached)".into()));
}

#[gpui_kit::test]
fn opens_the_worktree_a_branch_is_checked_out_in_when_its_label_is_double_clicked(
    cx: &mut TestAppContext,
) {
    let history = render(cx, with_worktrees());
    history.hover_more("e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5", cx);
    let feature = history.label(
        "e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5",
        "refs/heads/feature",
    );
    cx.update_window(history.window, |_, window, cx| {
        window.double_click(feature, cx)
    })
    .unwrap();
    cx.run_until_parked();
    assert!(history.calls.switched.borrow().is_empty());
    assert_eq!(history.opened_worktrees(), ["/w/repo-feature"]);
}

#[gpui_kit::test]
fn opens_the_worktree_a_branch_is_checked_out_in_from_its_menu(cx: &mut TestAppContext) {
    let history = render(cx, with_worktrees());
    history.hover_more("e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5", cx);
    history.menu_of(
        history.label(
            "e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5",
            "refs/heads/feature",
        ),
        cx,
    );
    history.choose("Open worktree repo-feature", cx);
    assert_eq!(history.opened_worktrees(), ["/w/repo-feature"]);
}

#[gpui_kit::test]
fn adds_a_worktree_for_a_branch_from_its_menu(cx: &mut TestAppContext) {
    let history = render(cx, with_worktrees());
    history.menu_of(
        history.label(
            "e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5",
            "refs/heads/main",
        ),
        cx,
    );
    history.choose("Add worktree…", cx);
    let provider = history.host.read_with(cx, |host, _| host.provider.clone());
    assert!(provider.read_with(cx, |provider, cx| {
        provider.add_worktree_dialog().read(cx).is_open()
    }));
}

// --- history-graph.test.tsx ------------------------------------------------------------------

impl Rendered {
    fn hover_node(&self, sha: &str, cx: &mut TestAppContext) {
        cx.update_window(self.window, |_, window, cx| {
            window.hover(part(&keyed("graph", sha.to_owned()), "node"), cx)
        })
        .unwrap();
    }

    fn has_author_tooltip(&self, cx: &mut TestAppContext) -> bool {
        cx.update_window(self.window, |_, window, cx| {
            window.render_frame(cx);
            window.try_find("author-tooltip").is_some()
        })
        .unwrap()
    }
}

#[gpui_kit::test]
fn shows_the_author_in_a_tooltip_while_the_commits_node_is_hovered(cx: &mut TestAppContext) {
    let history = render(cx, Start::default());
    assert!(!history.has_author_tooltip(cx));

    history.hover_node("d4", cx);
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert!(history.has_author_tooltip(cx));

    // Moving away from the node hides it.
    cx.update_window(history.window, |_, window, cx| {
        window.hover(keyed("commit", "a1"), cx)
    })
    .unwrap();
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert!(!history.has_author_tooltip(cx));
}

#[gpui_kit::test]
fn doesnt_show_the_tooltip_of_a_node_the_pointer_only_passed_over(cx: &mut TestAppContext) {
    let history = render(cx, Start::default());
    history.hover_node("d4", cx);
    cx.update_window(history.window, |_, window, cx| {
        window.hover(keyed("commit", "a1"), cx)
    })
    .unwrap();
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert!(!history.has_author_tooltip(cx));
}

// --- commit-selection.test.tsx: the history's part ------------------------------------------

#[gpui_kit::test]
fn lists_the_uncommitted_changes_then_the_stashes_among_the_commits(cx: &mut TestAppContext) {
    let mut status = on_main("b1", 0);
    status.counts.unstaged = 1;
    status.counts.files = 1;
    let mut second = commit("b1", "Second", &["a1"], vec![]);
    second.committed_at = 2000;
    let mut first = commit("a1", "First", &[], vec![]);
    first.committed_at = 1000;
    let newer = Stash {
        sha: "d".repeat(40),
        base: "b1".into(),
        message: "On main: newer stash".into(),
        created_at: 6000,
    };
    let older = Stash {
        sha: "c".repeat(40),
        base: "a1".into(),
        message: "WIP on main: a1 First".into(),
        created_at: 5000,
    };
    let history = render(
        cx,
        Start {
            log: vec![second, first],
            status,
            stashes: vec![newer, older],
            ..Start::default()
        },
    );
    // Only the rows in view are rendered, so each says where it is in the whole list.
    let options: Vec<String> = history
        .options(cx)
        .into_iter()
        .map(|(label, _)| label)
        .collect();
    assert_eq!(
        options,
        [
            "Uncommitted changes",
            "On main: newer stash",
            "Second, by Ada Lovelace",
            "WIP on main: a1 First",
            "First, by Ada Lovelace",
        ]
    );
    let positions = cx
        .update_window(history.window, |_, window, _| {
            window
                .find_all_by_role(Role::ListBoxOption)
                .iter()
                .map(|option| option.bounds().origin.y)
                .collect::<Vec<_>>()
        })
        .unwrap();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));

    // A stash's row is selected by its id.
    cx.update_window(history.window, |_, window, cx| {
        window.click(keyed("stash", stash_row_id(&"c".repeat(40))), cx)
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(history.selected(cx), Some(stash_row_id(&"c".repeat(40))));
}
