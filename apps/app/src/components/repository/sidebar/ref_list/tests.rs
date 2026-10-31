use std::cell::{Cell, RefCell};

use futures::channel::oneshot;
use gitto_git::features::branch::schema::{MergeOutcome, RebaseOutcome};
use gitto_git::features::status::schema::{Head, StatusCounts};
use gpui_kit::component::WindowExt as _;
use gpui_kit::test::{TestEventExt as _, TestQueryExt as _, TestWindowExt as _};
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, MouseButton, TestAppContext, px};

use super::*;
use crate::components::repository::add_worktree_dialog::AddWorktreeDialog;
use crate::components::repository::create_branch_dialog::CreateBranchDialog;
use crate::components::repository::test_support::{
    fake_git, repository, repository_id, set_operation, set_push_target, set_refs, set_remotes,
    set_stashes, set_status, set_worktrees, setup_with, uncommitted,
};
use crate::components::repository::toolbar::RepositoryToolbar;
use futures::FutureExt as _;
use futures::future::BoxFuture;
use gitto_app_data::data::DataError;
use gitto_app_data::data::branch::{Merge, NewBranchFrom, Rebase};
use gitto_app_data::data::git::{GitTask, RepositoryGit};
use gitto_app_data::data::repositories::RepositorySource;
use gitto_app_data::data::worktree::AddWorktreeInput;
use gitto_app_data::data::{Change, PopFrom};
use gitto_app_data::state::router::{Route, Router};
use gitto_git::features::status::schema::Uncommitted;
use gitto_git::input::{BranchName, BranchRef, BranchShortName, Folder, FullSha, RepositoryId};
use gitto_repository::Repository;
use std::path::PathBuf;

/// A made-up operation: it records what it's asked, then goes through, fails saying `fail`,
/// or, while `hold` is set, waits to be finished.
struct Fake<T, R = ()> {
    calls: RefCell<Vec<T>>,
    hold: Cell<bool>,
    fail: RefCell<Option<String>>,
    pending: RefCell<Vec<oneshot::Sender<Result<R, DataError>>>>,
}

impl<T: Clone + 'static, R: Clone + Send + 'static> Fake<T, R> {
    fn new() -> Rc<Self> {
        Rc::new(Fake {
            calls: RefCell::new(Vec::new()),
            hold: Cell::new(false),
            fail: RefCell::new(None),
            pending: RefCell::new(Vec::new()),
        })
    }

    fn run(&self, input: T, done: R) -> GitTask<R> {
        self.calls.borrow_mut().push(input);
        if let Some(message) = self.fail.borrow().clone() {
            return async move { Err(DataError::msg(message)) }.boxed();
        }
        if self.hold.get() {
            let (sender, receiver) = oneshot::channel();
            self.pending.borrow_mut().push(sender);
            return async move { receiver.await.unwrap_or(Ok(done)) }.boxed();
        }
        async move { Ok(done) }.boxed()
    }

    fn calls(&self) -> Vec<T> {
        self.calls.borrow().clone()
    }

    fn fail_with(&self, message: &str) {
        *self.fail.borrow_mut() = Some(message.to_owned());
    }

    fn finish(&self, outcome: R) {
        for pending in self.pending.borrow_mut().drain(..) {
            let _ = pending.send(Ok(outcome.clone()));
        }
    }
}

/// Every operation the list, the branch menus and the toolbar's current branch run here.
struct Fakes {
    switch: Rc<Fake<String>>,
    create: Rc<Fake<NewBranchFrom>>,
    merge: Rc<Fake<Merge, MergeOutcome>>,
    rebase: Rc<Fake<Rebase, RebaseOutcome>>,
    delete: Rc<Fake<String>>,
    pull: Rc<Fake<()>>,
    pull_branch: Rc<Fake<String>>,
    pop: Rc<Fake<String>>,
    drop: Rc<Fake<String>>,
    add: Rc<Fake<AddWorktreeInput>>,
    remove: Rc<Fake<String>>,
    open: Rc<Fake<String>>,
}

impl Fakes {
    fn new() -> Self {
        Fakes {
            switch: Fake::new(),
            create: Fake::new(),
            merge: Fake::new(),
            rebase: Fake::new(),
            delete: Fake::new(),
            pull: Fake::new(),
            pull_branch: Fake::new(),
            pop: Fake::new(),
            drop: Fake::new(),
            add: Fake::new(),
            remove: Fake::new(),
            open: Fake::new(),
        }
    }
}

/// The repository's git, as the list, the branch menus and the toolbar ask it.
impl RepositoryGit for Fakes {
    fn switch_branch(&self, reference: BranchRef) -> GitTask<()> {
        self.switch.run(reference.as_str().to_owned(), ())
    }

    fn create_branch(&self, name: BranchName, from: Option<BranchRef>) -> GitTask<()> {
        let input = NewBranchFrom {
            name,
            from: from.expect("a branch made from another"),
        };
        self.create.run(input, ())
    }

    fn merge_branch(&self, reference: BranchRef, into: BranchShortName) -> GitTask<MergeOutcome> {
        let input = Merge { reference, into };
        self.merge.run(input, MergeOutcome::Merged)
    }

    fn rebase_branch(
        &self,
        reference: BranchRef,
        branch: BranchShortName,
    ) -> GitTask<RebaseOutcome> {
        let input = Rebase { reference, branch };
        self.rebase.run(input, RebaseOutcome::Rebased)
    }

    fn delete_branch(&self, reference: BranchRef) -> GitTask<()> {
        self.delete.run(reference.as_str().to_owned(), ())
    }

    fn pull(&self, branch: Option<BranchShortName>) -> GitTask<()> {
        match branch {
            Some(branch) => self.pull_branch.run(branch.as_str().to_owned(), ()),
            None => self.pull.run((), ()),
        }
    }

    fn pop_stash(&self, sha: FullSha) -> GitTask<()> {
        self.pop.run(sha.as_str().to_owned(), ())
    }

    fn drop_stash(&self, sha: FullSha) -> GitTask<()> {
        self.drop.run(sha.as_str().to_owned(), ())
    }

    fn add_worktree(
        &self,
        folder: Folder,
        branch: BranchRef,
        new_branch: Option<BranchName>,
    ) -> GitTask<()> {
        let input = AddWorktreeInput {
            path: folder,
            branch,
            new_branch,
        };
        self.add.run(input, ())
    }

    fn remove_worktree(&self, path: String) -> GitTask<()> {
        self.remove.run(path, ())
    }
}

/// The id the worktree opened gets, added to Gitto as a repository of its own.
fn worktree_id() -> RepositoryId {
    RepositoryId::parse("01890a5d-ac96-774b-bcce-b302099a8058").unwrap()
}

/// The repositories added to Gitto; opening a worktree adds it, as `open` answers.
struct Source {
    list: RefCell<Vec<Repository>>,
    open: Rc<Fake<String>>,
}

impl RepositorySource for Source {
    fn list(&self) -> BoxFuture<'static, Result<Vec<Repository>, DataError>> {
        let list = self.list.borrow().clone();
        async move { Ok(list) }.boxed()
    }

    fn add(&self, path: PathBuf) -> BoxFuture<'static, Result<Repository, DataError>> {
        let path = path.to_string_lossy().into_owned();
        let opened = repository(&worktree_id(), &path);
        self.open
            .run(path, ())
            .map(move |result| result.map(|()| opened))
            .boxed()
    }

    fn remove(&self, _: RepositoryId) -> BoxFuture<'static, Result<(), DataError>> {
        async { Err(DataError::msg("Not in this test.")) }.boxed()
    }

    fn reorder(&self, _: Vec<RepositoryId>) -> BoxFuture<'static, Result<(), DataError>> {
        async { Err(DataError::msg("Not in this test.")) }.boxed()
    }
}

/// The toolbar, whose current branch shows a switch or a merge running and why it failed,
/// and the list, with the branch menus they share.
struct Host {
    toolbar: Entity<RepositoryToolbar>,
    provider: Entity<BranchMenuProvider>,
    list: Entity<RefList>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .child(self.toolbar.clone())
            .child(div().w(px(280.)).h(px(640.)).child(self.list.clone()))
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    id: RepositoryId,
    other: RepositoryId,
    window: AnyWindowHandle,
    host: Entity<Host>,
    fakes: Rc<Fakes>,
}

fn local(name: &str, current: bool) -> Ref {
    Ref {
        name: name.into(),
        full_name: format!("refs/heads/{name}"),
        kind: RefKind::Local,
        sha: "abc".into(),
        current,
        upstream: None,
        ahead: 0,
        behind: 0,
    }
}

fn remote(name: &str) -> Ref {
    Ref {
        kind: RefKind::Remote,
        full_name: format!("refs/remotes/{name}"),
        ..local(name, false)
    }
}

/// `name`, a local branch, tracking `origin/<name>`.
fn tracking(name: &str, current: bool) -> Ref {
    Ref {
        upstream: Some(format!("origin/{name}")),
        ..local(name, current)
    }
}

fn tag(name: &str) -> Ref {
    Ref {
        kind: RefKind::Tag,
        full_name: format!("refs/tags/{name}"),
        ..local(name, false)
    }
}

/// On main, with a change.
fn status() -> Uncommitted {
    let mut status = uncommitted(Head::Branch {
        name: "main".into(),
        sha: "abc".into(),
    });
    status.summary.counts = StatusCounts {
        files: 1,
        staged: 0,
        unstaged: 1,
        conflicted: 0,
    };
    status
}

fn stash(sha: char, message: &str, created_at: i64) -> Stash {
    Stash {
        sha: sha.to_string().repeat(40),
        base: "abc".into(),
        message: message.into(),
        created_at,
    }
}

fn newer() -> Stash {
    stash('b', "On main: newer", 2)
}

fn older() -> Stash {
    stash('a', "On main: older", 1)
}

fn worktree(path: &str, branch: Option<&str>) -> Worktree {
    Worktree {
        path: path.into(),
        name: path.rsplit('/').next().unwrap().into(),
        head: Some("abc".into()),
        branch: branch.map(Into::into),
        main: false,
        bare: false,
        current: false,
        locked: None,
        prunable: None,
    }
}

/// The repository's worktrees: the main one, on show, on `main`; one on `feature`; one
/// detached; and one whose folder is gone.
fn worktrees() -> Vec<Worktree> {
    vec![
        Worktree {
            main: true,
            current: true,
            ..worktree("/w/repo", Some("refs/heads/main"))
        },
        worktree("/w/repo-feature", Some("refs/heads/feature")),
        worktree("/w/repo-check", None),
        Worktree {
            prunable: Some("gitdir file points to non-existent location".into()),
            ..worktree("/w/repo-gone", None)
        },
    ]
}

/// Gives `id` what's on show, so nothing loads from git.
fn seed(id: &RepositoryId, cx: &mut TestAppContext) {
    set_status(id, status(), cx);
    set_stashes(id, vec![], cx);
    set_push_target(
        id,
        gitto_git::features::remote::schema::PushTarget::Blocked {
            reason: gitto_git::PushBlocker::NoRemote,
        },
        cx,
    );
    set_operation(id, None, cx);
    set_refs(
        id,
        vec![
            local("main", true),
            local("feature", false),
            remote("origin/feature"),
        ],
        cx,
    );
    set_worktrees(id, vec![], cx);
    set_remotes(id, &["origin"], cx);
}

/// The toolbar and the list of the repository at /w/repo, `prepare` changing what's on show
/// first.
fn render_with(
    cx: &mut TestAppContext,
    prepare: impl FnOnce(&RepositoryId, &mut TestAppContext),
) -> Fixture {
    let id = repository_id();
    let other = repository_id();
    let fakes = Rc::new(Fakes::new());
    let repositories = Rc::new(Source {
        list: RefCell::new(vec![
            repository(&id, "/w/repo"),
            repository(&other, "/w/other"),
        ]),
        open: Rc::clone(&fakes.open),
    });
    let dir = setup_with(cx, repositories);
    fake_git(&id, fakes.clone(), cx);
    seed(&id, cx);
    seed(&other, cx);
    prepare(&id, cx);
    let shown = id.clone();
    let (window, host) = crate::test_support::open(cx, move |window, cx| {
        let create = cx.new(|cx| CreateBranchDialog::new(shown.clone(), window, cx));
        let add = cx.new(|cx| AddWorktreeDialog::new(shown.clone(), window, cx));
        let provider = cx.new(|cx| BranchMenuProvider::with(shown.clone(), create, add, cx));
        let toolbar = cx.new(|cx| RepositoryToolbar::new(shown.clone(), window, cx));
        let list = cx.new(|cx| RefList::new(shown.clone(), provider.clone(), window, cx));
        Host {
            toolbar,
            provider,
            list,
        }
    });
    cx.run_until_parked();
    Fixture {
        _dir: dir,
        id,
        other,
        window,
        host,
        fakes,
    }
}

fn render(cx: &mut TestAppContext) -> Fixture {
    render_with(cx, |_, _| {})
}

impl Fixture {
    fn list(&self, cx: &mut TestAppContext) -> Entity<RefList> {
        self.host.read_with(cx, |host, _| host.list.clone())
    }

    /// The repository on show's entity.
    fn repository(&self, cx: &mut TestAppContext) -> Entity<gitto_app_data::data::Repository> {
        let list = self.list(cx);
        list.read_with(cx, |list, _| list.repository.entity().clone())
    }

    fn provider(&self, cx: &mut TestAppContext) -> Entity<BranchMenuProvider> {
        self.host.read_with(cx, |host, _| host.provider.clone())
    }

    fn in_window<R>(
        &self,
        cx: &mut TestAppContext,
        run: impl FnOnce(&mut Window, &mut App) -> R,
    ) -> R {
        let result = cx
            .update_window(self.window, |_, window, cx| run(window, cx))
            .unwrap();
        cx.run_until_parked();
        result
    }

    fn double_click(&self, id: &str, cx: &mut TestAppContext) {
        let id = SharedString::from(id.to_owned());
        self.in_window(cx, |window, cx| window.double_click(id, cx));
    }

    fn click(&self, id: &str, cx: &mut TestAppContext) {
        let id = SharedString::from(id.to_owned());
        self.in_window(cx, |window, cx| window.click(id, cx));
    }

    /// Right-clicks the row `id`, and reads the labels of the menu it opens, if any.
    fn open_menu(&self, id: &str, cx: &mut TestAppContext) -> Vec<String> {
        let id = SharedString::from(id.to_owned());
        self.in_window(cx, |window, cx| window.right_click(id, cx));
        self.in_window(cx, |window, _| {
            if window.try_find("popup-menu").is_none() {
                return Vec::new();
            }
            window
                .within("popup-menu")
                .find_all_by_role(gpui_kit::Role::MenuItem)
                .into_iter()
                .map(|item| item.label().unwrap_or_default().to_owned())
                .collect()
        })
    }

    fn is_menu_open(&self, cx: &mut TestAppContext) -> bool {
        self.in_window(cx, |window, _| window.try_find("popup-menu").is_some())
    }

    /// Clicks what's labelled `label`: a menu's item, or a dialog's button.
    fn choose(&self, label: &str, cx: &mut TestAppContext) {
        self.in_window(cx, |window, cx| {
            let center = window.find_by_label(label).bounds().center();
            window.pointer_down(center, MouseButton::Left, cx);
            window.pointer_up(center, MouseButton::Left, cx);
        });
    }

    fn press(&self, key: &str, cx: &mut TestAppContext) {
        self.in_window(cx, |window, cx| window.press(key, cx));
    }

    fn has_dialog(&self, cx: &mut TestAppContext) -> bool {
        self.in_window(cx, |window, cx| window.has_active_dialog(cx))
    }

    /// Whether the toolbar's current branch shows a change of the branches running.
    fn is_busy(&self, cx: &mut TestAppContext) -> bool {
        let toolbar = self.host.read_with(cx, |host, _| host.toolbar.clone());
        toolbar.read_with(cx, |toolbar, cx| toolbar.is_branch_busy(cx))
    }

    /// The failure the toolbar's current branch shows, by its title and message.
    fn branch_failure(&self, cx: &mut TestAppContext) -> Option<(&'static str, String)> {
        let toolbar = self.host.read_with(cx, |host, _| host.toolbar.clone());
        toolbar.read_with(cx, |toolbar, _| {
            toolbar
                .branch_failure()
                .map(|(kind, error)| (kind.title(), error.to_string()))
        })
    }

    fn is_section_busy(&self, section: Section, cx: &mut TestAppContext) -> bool {
        let list = self.list(cx);
        list.read_with(cx, |list, cx| list.is_section_busy(section, cx))
    }

    /// The failure shown beside the section, by its title and message.
    fn section_failure(
        &self,
        section: Section,
        cx: &mut TestAppContext,
    ) -> Option<(String, String)> {
        let list = self.list(cx);
        list.read_with(cx, |list, cx| {
            let repository = list.repository.entity();
            let failure = match section {
                Section::Stashes => StashMenu::failure(repository, cx),
                Section::Worktrees => WorktreeMenu::failure(repository, cx),
                _ => unreachable!("{section:?} has no actions on its items"),
            };
            failure
                .error
                .map(|error| (failure.title.to_string(), error.to_string()))
        })
    }

    /// Shows the repository `id` instead, as the page does.
    fn show(&self, id: &RepositoryId, cx: &mut TestAppContext) {
        let list = self.list(cx);
        let provider = self.provider(cx);
        let host = self.host.clone();
        let id = id.clone();
        self.in_window(cx, |window, cx| {
            provider.update(cx, |provider, cx| {
                provider.set_repository(id.clone(), window, cx)
            });
            // The page makes another toolbar for another repository.
            let toolbar = cx.new(|cx| RepositoryToolbar::new(id.clone(), window, cx));
            host.update(cx, |host, cx| {
                host.toolbar = toolbar;
                cx.notify();
            });
            list.update(cx, |list, cx| list.set_repository(id, window, cx));
        });
    }

    /// Expands `section` if it's collapsed (tags and stashes are, at first).
    fn expand(&self, section: Section, cx: &mut TestAppContext) {
        let list = self.list(cx);
        if list.read_with(cx, |list, _| list.is_section_collapsed(section)) {
            self.click(&format!("section-{}", section.key()), cx);
        }
    }

    fn has_row(&self, id: &str, cx: &mut TestAppContext) -> bool {
        let id = SharedString::from(id.to_owned());
        self.in_window(cx, |window, cx| {
            window.render_frame(cx);
            window.try_find(id).is_some()
        })
    }

    fn run_on_list(
        &self,
        cx: &mut TestAppContext,
        run: impl FnOnce(&mut RefList, &mut Context<RefList>),
    ) {
        let list = self.list(cx);
        list.update(cx, run);
        cx.run_until_parked();
    }
}

const FEATURE: &str = "branch-refs/heads/feature";
const MAIN: &str = "branch-refs/heads/main";
const ORIGIN_FEATURE: &str = "branch-refs/remotes/origin/feature";

fn stash_row(stash: &Stash) -> String {
    format!("stash-{}", stash.sha)
}

fn worktree_row(path: &str) -> String {
    format!("worktree-{path}")
}

fn items(labels: &[&str]) -> Vec<String> {
    labels.iter().map(|label| (*label).to_owned()).collect()
}

// What the rows say.

#[test]
fn lays_branches_out_in_folders_leaving_out_the_collapsed_ones() {
    let refs = [
        local("feature/login", false),
        local("feature/search", false),
        local("main", true),
    ];
    let lines = tree_lines(&refs, |_| false);
    let names: Vec<_> = lines
        .iter()
        .map(|line| match line {
            TreeLine::Folder { name, depth, .. } => format!("{depth} {name}/"),
            TreeLine::Branch { label, depth, .. } => format!("{depth} {label}"),
        })
        .collect();
    assert_eq!(names, ["0 feature/", "1 login", "1 search", "0 main"]);

    let collapsed = tree_lines(&refs, |path| path == "refs/heads/feature");
    assert_eq!(collapsed.len(), 2);
    assert!(matches!(
        &collapsed[0],
        TreeLine::Folder {
            collapsed: true,
            count: 2,
            ..
        }
    ));
}

#[test]
fn says_how_far_a_branch_is_ahead_or_else_behind() {
    let ahead = Ref {
        ahead: 2,
        behind: 1,
        ..local("main", true)
    };
    assert_eq!(branch_meta(&ahead).as_deref(), Some("↑2"));
    let behind = Ref {
        behind: 1,
        ..local("main", true)
    };
    assert_eq!(branch_meta(&behind).as_deref(), Some("↓1"));
    assert_eq!(branch_meta(&local("main", true)), None);
}

#[test]
fn says_which_other_worktree_a_branch_is_checked_out_in() {
    let all = worktrees();
    assert_eq!(
        branch_title(&local("feature", false), Some(&all[1])),
        "feature\nChecked out in /w/repo-feature"
    );
    assert_eq!(branch_title(&local("main", true), None), "main");
}

#[test]
fn says_when_a_stash_was_made() {
    gitto_ui_logic::format::tell_relative_times_in("en");
    let now = 10 * 60 * 1000;
    let made = stash('a', "On main: try", now - 3 * 60 * 1000);
    assert_eq!(
        stash_title(&made, now),
        "On main: try\nStashed 3 minutes ago"
    );
}

#[test]
fn says_what_a_worktree_is_on() {
    let all = worktrees();
    assert_eq!(worktree_meta(&all[1]), "feature");
    assert_eq!(worktree_meta(&all[2]), "detached");
    let bare = Worktree {
        bare: true,
        ..worktree("/w/bare", None)
    };
    assert_eq!(worktree_meta(&bare), "bare");
    assert_eq!(describe_worktree(&all[0]), "/w/repo\nOn main\nOpen here");
}

// ref-list.test.tsx: reloading the repository.

#[gpui_kit::test]
fn keeps_the_sections_on_the_page_so_their_lists_stay_where_theyre_scrolled_to(
    cx: &mut TestAppContext,
) {
    let many: Vec<Ref> = (0..60)
        .map(|ix| local(&format!("b{ix:02}"), false))
        .collect();
    let f = render_with(cx, |id, cx| set_refs(id, many.clone(), cx));
    let list = f.list(cx);
    let scroll = list.read_with(cx, |list, _| list.scrolls[0].clone());
    scroll.scroll_to_item_strict(40, ScrollStrategy::Top);
    f.in_window(cx, |window, cx| window.render_frame(cx));
    assert!(f.has_row("branch-refs/heads/b45", cx));
    assert!(!f.has_row("branch-refs/heads/b00", cx));

    // A reload, which brought something new.
    let mut reloaded = many;
    reloaded[0].sha = "def".into();
    set_refs(&f.id, reloaded, cx);
    assert!(f.has_row("branch-refs/heads/b45", cx));
    assert!(!f.has_row("branch-refs/heads/b00", cx));
}

#[gpui_kit::test]
fn starts_another_repositorys_lists_at_the_top(cx: &mut TestAppContext) {
    let many: Vec<Ref> = (0..60)
        .map(|ix| local(&format!("b{ix:02}"), false))
        .collect();
    let f = render_with(cx, |id, cx| set_refs(id, many.clone(), cx));
    set_refs(&f.other, many, cx);
    let list = f.list(cx);
    let scroll = list.read_with(cx, |list, _| list.scrolls[0].clone());
    scroll.scroll_to_item_strict(40, ScrollStrategy::Top);
    f.in_window(cx, |window, cx| window.render_frame(cx));
    assert!(!f.has_row("branch-refs/heads/b00", cx));

    f.show(&f.other.clone(), cx);
    assert!(f.has_row("branch-refs/heads/b00", cx));
}

#[gpui_kit::test]
fn starts_with_the_tags_and_stashes_collapsed(cx: &mut TestAppContext) {
    let f = render(cx);
    assert!(f.has_row(MAIN, cx));
    let list = f.list(cx);
    assert!(list.read_with(cx, |list, _| list.is_section_collapsed(Section::Tags)));
    assert!(list.read_with(cx, |list, _| list.is_section_collapsed(Section::Stashes)));
    assert!(!list.read_with(cx, |list, _| list.is_section_collapsed(Section::Local)));
    assert!(f.in_window(cx, |window, _| {
        window.find("section-tags").expanded() == Some(false)
            && window.find("section-local").expanded() == Some(true)
    }));
}

#[gpui_kit::test]
fn collapses_a_section_and_a_folder_and_keeps_it_so(cx: &mut TestAppContext) {
    let f = render_with(cx, |id, cx| {
        set_refs(
            id,
            vec![
                local("main", true),
                remote("origin/feature"),
                remote("origin/main"),
            ],
            cx,
        )
    });
    assert!(f.has_row(ORIGIN_FEATURE, cx));
    f.click("folder-refs/remotes/origin", cx);
    assert!(!f.has_row(ORIGIN_FEATURE, cx));
    assert!(f.has_row("folder-refs/remotes/origin", cx));
    f.click("folder-refs/remotes/origin", cx);
    assert!(f.has_row(ORIGIN_FEATURE, cx));

    f.click("section-local", cx);
    assert!(!f.has_row(MAIN, cx));
    // Another repository has its own; this one's is kept for when it's back.
    f.show(&f.other.clone(), cx);
    assert!(f.has_row(MAIN, cx));
    f.show(&f.id.clone(), cx);
    assert!(!f.has_row(MAIN, cx));
}

#[gpui_kit::test]
fn lists_the_tags_which_have_no_menu(cx: &mut TestAppContext) {
    let f = render_with(cx, |id, cx| {
        set_refs(id, vec![local("main", true), tag("v0.1.0")], cx)
    });
    assert!(!f.has_row("tag-refs/tags/v0.1.0", cx));
    f.expand(Section::Tags, cx);
    assert!(f.has_row("tag-refs/tags/v0.1.0", cx));
    assert!(f.open_menu("tag-refs/tags/v0.1.0", cx).is_empty());
}

// ref-list.test.tsx: switching branches from the sidebar.

#[gpui_kit::test]
fn switches_to_a_branch_thats_double_clicked_showing_it_running(cx: &mut TestAppContext) {
    let f = render(cx);
    f.fakes.switch.hold.set(true);

    f.click(FEATURE, cx);
    assert!(f.fakes.switch.calls().is_empty());
    f.double_click(FEATURE, cx);
    assert_eq!(f.fakes.switch.calls(), ["refs/heads/feature"]);
    assert!(f.is_busy(cx));

    f.fakes.switch.finish(());
    cx.run_until_parked();
    assert!(!f.is_busy(cx));
}

#[gpui_kit::test]
fn switches_to_a_remote_branch_thats_double_clicked(cx: &mut TestAppContext) {
    let f = render(cx);
    f.double_click(ORIGIN_FEATURE, cx);
    assert_eq!(f.fakes.switch.calls(), ["refs/remotes/origin/feature"]);
}

#[gpui_kit::test]
fn doesnt_switch_to_the_current_branch(cx: &mut TestAppContext) {
    let f = render(cx);
    // The switch does nothing for the checked-out branch rather than run git.
    f.double_click(MAIN, cx);
    assert!(f.fakes.switch.calls().is_empty());
    assert!(!f.is_busy(cx));
    assert_eq!(f.branch_failure(cx), None);
}

#[gpui_kit::test]
fn says_when_the_changes_were_kept_in_the_stash(cx: &mut TestAppContext) {
    let f = render(cx);
    let message = "Switched to feature, but your uncommitted changes conflict with it, so \
                   they're kept in the stash.";
    f.fakes.switch.fail_with(message);

    f.double_click(FEATURE, cx);
    assert_eq!(
        f.branch_failure(cx),
        Some(("Switch branch", message.to_owned()))
    );

    f.choose("Dismiss", cx);
    assert_eq!(f.branch_failure(cx), None);
}

// ref-list.test.tsx: the branch menu.

fn create_from(f: &Fixture, row: &str, cx: &mut TestAppContext) {
    assert!(f.open_menu(row, cx).contains(&"Create branch…".to_owned()));
    f.choose("Create branch…", cx);
}

fn create_dialog(f: &Fixture, cx: &mut TestAppContext) -> Entity<CreateBranchDialog> {
    let provider = f.provider(cx);
    provider.read_with(cx, |provider, _| provider.create_dialog().clone())
}

#[gpui_kit::test]
fn creates_a_branch_from_the_one_right_clicked_and_switches_to_it_as_a_switch(
    cx: &mut TestAppContext,
) {
    let f = render(cx);
    f.fakes.create.hold.set(true);

    create_from(&f, FEATURE, cx);
    assert!(f.has_dialog(cx));
    let dialog = create_dialog(&f, cx);
    assert_eq!(
        dialog.read_with(cx, |dialog, cx| dialog.summary(cx)),
        "From feature, with your uncommitted changes, unless they conflict with it: then \
         they're kept in the stash."
    );

    f.in_window(cx, |window, cx| window.input("  topic  ", cx));
    f.press("enter", cx);
    assert_eq!(
        f.fakes.create.calls(),
        [NewBranchFrom::parse("topic", "refs/heads/feature").unwrap()]
    );
    // The dialog closes, and the current branch shows it running.
    assert!(!f.has_dialog(cx));
    assert!(f.is_busy(cx));

    // Nor another switch meanwhile.
    f.open_menu(MAIN, cx);
    f.choose("Create branch…", cx);
    assert!(!f.has_dialog(cx));
    f.double_click(ORIGIN_FEATURE, cx);
    assert!(f.fakes.switch.calls().is_empty());

    f.fakes.create.finish(());
    cx.run_until_parked();
    assert!(!f.is_busy(cx));
}

#[gpui_kit::test]
fn doesnt_create_one_while_switching(cx: &mut TestAppContext) {
    let f = render(cx);
    f.fakes.switch.hold.set(true);

    // The dialog was open before the switch began.
    create_from(&f, MAIN, cx);
    f.in_window(cx, |window, cx| window.input("topic", cx));
    // Begun straight on the list: the dialog keeps the sidebar from being used, so this
    // stands for a switch begun some other way.
    f.run_on_list(cx, |list, cx| {
        list.activate_branch("refs/heads/feature", cx)
    });
    assert!(f.is_busy(cx));
    f.press("enter", cx);
    assert!(f.fakes.create.calls().is_empty());

    f.fakes.switch.finish(());
    cx.run_until_parked();
    assert!(!f.is_busy(cx));
    assert!(f.fakes.create.calls().is_empty());
}

#[gpui_kit::test]
fn says_the_changes_come_along_from_the_current_branch(cx: &mut TestAppContext) {
    let f = render(cx);
    create_from(&f, MAIN, cx);
    let dialog = create_dialog(&f, cx);
    assert_eq!(
        dialog.read_with(cx, |dialog, cx| dialog.summary(cx)),
        "From main, with your uncommitted changes."
    );
}

#[gpui_kit::test]
fn creates_one_from_a_remote_branch(cx: &mut TestAppContext) {
    let f = render(cx);
    create_from(&f, ORIGIN_FEATURE, cx);
    f.in_window(cx, |window, cx| window.input("topic", cx));
    f.press("enter", cx);
    assert_eq!(
        f.fakes.create.calls(),
        [NewBranchFrom::parse("topic", "refs/remotes/origin/feature").unwrap()]
    );
}

#[gpui_kit::test]
fn opens_nothing_for_what_isnt_a_branch(cx: &mut TestAppContext) {
    let f = render(cx);
    assert!(f.open_menu("folder-refs/remotes/origin", cx).is_empty());
    assert!(!f.is_menu_open(cx));
    assert!(!f.open_menu(FEATURE, cx).is_empty());
    f.press("escape", cx);
    assert!(!f.is_menu_open(cx));
    assert!(f.open_menu("folder-refs/remotes/origin", cx).is_empty());
}

#[gpui_kit::test]
fn says_why_a_branch_couldnt_be_created_and_keeps_the_name_to_fix(cx: &mut TestAppContext) {
    let f = render(cx);
    let message = "fatal: a branch named 'topic' already exists";
    f.fakes.create.fail_with(message);

    create_from(&f, FEATURE, cx);
    f.in_window(cx, |window, cx| window.input("topic", cx));
    f.press("enter", cx);
    assert_eq!(
        f.branch_failure(cx),
        Some(("Switch branch", message.to_owned()))
    );

    create_from(&f, FEATURE, cx);
    let dialog = create_dialog(&f, cx);
    let name = dialog.read_with(cx, |dialog, _| dialog.name().clone());
    assert_eq!(
        name.read_with(cx, |name, _| name.value().to_string()),
        "topic"
    );
    // Out of the way of the name.
    assert_eq!(f.branch_failure(cx), None);
}

// ref-list.test.tsx: merging and rebasing from the branch menu.

#[gpui_kit::test]
fn merges_the_branch_right_clicked_into_the_checked_out_one_showing_it_running(
    cx: &mut TestAppContext,
) {
    let f = render(cx);
    f.fakes.merge.hold.set(true);

    assert!(
        f.open_menu(FEATURE, cx)
            .contains(&"Merge feature into main".to_owned())
    );
    f.choose("Merge feature into main", cx);
    assert_eq!(
        f.fakes.merge.calls(),
        [Merge::parse("refs/heads/feature", "main").unwrap()]
    );
    assert!(f.is_busy(cx));

    // Nor another merge, or a switch, meanwhile.
    f.open_menu(ORIGIN_FEATURE, cx);
    f.choose("Merge origin/feature into main", cx);
    assert_eq!(f.fakes.merge.calls().len(), 1);
    f.double_click(ORIGIN_FEATURE, cx);
    assert!(f.fakes.switch.calls().is_empty());

    f.fakes.merge.finish(MergeOutcome::Merged);
    cx.run_until_parked();
    assert!(!f.is_busy(cx));
    assert_eq!(f.branch_failure(cx), None);
}

#[gpui_kit::test]
fn merges_a_remote_branch(cx: &mut TestAppContext) {
    let f = render(cx);
    f.open_menu(ORIGIN_FEATURE, cx);
    f.choose("Merge origin/feature into main", cx);
    assert_eq!(
        f.fakes.merge.calls(),
        [Merge::parse("refs/remotes/origin/feature", "main").unwrap()]
    );
}

#[gpui_kit::test]
fn isnt_offered_for_the_checked_out_branch(cx: &mut TestAppContext) {
    let f = render(cx);
    let offered = f.open_menu(MAIN, cx);
    assert!(offered.contains(&"Create branch…".to_owned()));
    assert!(!offered.iter().any(|item| item.starts_with("Merge")));
    assert!(!offered.iter().any(|item| item.starts_with("Rebase")));
}

#[gpui_kit::test]
fn doesnt_switch_branches_or_create_one_while_it_runs(cx: &mut TestAppContext) {
    let f = render(cx);
    f.fakes.merge.hold.set(true);

    f.open_menu(FEATURE, cx);
    f.choose("Merge feature into main", cx);
    assert!(f.is_busy(cx));
    f.double_click(ORIGIN_FEATURE, cx);
    assert!(f.fakes.switch.calls().is_empty());
    f.open_menu(FEATURE, cx);
    f.choose("Create branch…", cx);
    assert!(!f.has_dialog(cx));

    f.fakes.merge.finish(MergeOutcome::Merged);
    cx.run_until_parked();
    f.double_click(ORIGIN_FEATURE, cx);
    assert_eq!(f.fakes.switch.calls(), ["refs/remotes/origin/feature"]);
}

#[gpui_kit::test]
fn shows_the_failure_that_came_last_a_switchs_after_a_merges(cx: &mut TestAppContext) {
    let f = render(cx);
    f.fakes.merge.fail_with("The merge failed.");
    f.fakes.switch.fail_with("The switch failed.");

    f.open_menu(FEATURE, cx);
    f.choose("Merge feature into main", cx);
    assert_eq!(
        f.branch_failure(cx),
        Some(("Merge", "The merge failed.".to_owned()))
    );
    // Begun straight on the list, as the failure's popover is in the way.
    f.run_on_list(cx, |list, cx| {
        list.activate_branch("refs/remotes/origin/feature", cx)
    });
    assert_eq!(
        f.branch_failure(cx),
        Some(("Switch branch", "The switch failed.".to_owned()))
    );

    // The merge's is still there once that's dismissed.
    f.choose("Dismiss", cx);
    assert_eq!(
        f.branch_failure(cx),
        Some(("Merge", "The merge failed.".to_owned()))
    );
}

#[gpui_kit::test]
fn isnt_offered_while_switching_branches(cx: &mut TestAppContext) {
    let f = render(cx);
    f.fakes.switch.hold.set(true);

    f.double_click(FEATURE, cx);
    assert!(f.is_busy(cx));
    f.open_menu(ORIGIN_FEATURE, cx);
    f.choose("Merge origin/feature into main", cx);
    assert!(f.fakes.merge.calls().is_empty());

    f.fakes.switch.finish(());
    cx.run_until_parked();
    assert!(!f.is_busy(cx));
}

#[gpui_kit::test]
fn isnt_offered_while_conflicts_are_left_to_resolve(cx: &mut TestAppContext) {
    let f = render_with(cx, |id, cx| {
        let mut conflicted = status();
        conflicted.summary.counts.conflicted = 1;
        set_status(id, conflicted, cx);
    });
    f.open_menu(FEATURE, cx);
    f.choose("Merge feature into main", cx);
    assert!(f.fakes.merge.calls().is_empty());
}

#[gpui_kit::test]
fn rebases_the_checked_out_branch_onto_the_one_right_clicked_showing_it_running(
    cx: &mut TestAppContext,
) {
    let f = render(cx);
    f.fakes.rebase.hold.set(true);

    f.open_menu(FEATURE, cx);
    f.choose("Rebase main onto feature", cx);
    assert_eq!(
        f.fakes.rebase.calls(),
        [Rebase::parse("refs/heads/feature", "main").unwrap()]
    );
    assert!(f.is_busy(cx));

    // Nor a merge, or another rebase, meanwhile.
    f.open_menu(ORIGIN_FEATURE, cx);
    f.choose("Merge origin/feature into main", cx);
    f.open_menu(ORIGIN_FEATURE, cx);
    f.choose("Rebase main onto origin/feature", cx);
    assert!(f.fakes.merge.calls().is_empty());
    assert_eq!(f.fakes.rebase.calls().len(), 1);

    f.fakes.rebase.finish(RebaseOutcome::Rebased);
    cx.run_until_parked();
    assert!(!f.is_busy(cx));
}

// ref-list.test.tsx: pulling from the branch menu.

fn pulling(id: &RepositoryId, cx: &mut TestAppContext) {
    set_refs(
        id,
        vec![
            tracking("main", true),
            tracking("feature", false),
            local("lonely", false),
            remote("origin/feature"),
        ],
        cx,
    );
}

#[gpui_kit::test]
fn pulls_a_branch_that_isnt_checked_out_showing_it_running(cx: &mut TestAppContext) {
    let f = render_with(cx, pulling);
    f.fakes.pull_branch.hold.set(true);

    assert!(
        f.open_menu(FEATURE, cx)
            .contains(&"Pull feature".to_owned())
    );
    f.choose("Pull feature", cx);
    assert_eq!(f.fakes.pull_branch.calls(), ["feature"]);
    assert!(f.is_busy(cx));

    f.fakes.pull_branch.finish(());
    cx.run_until_parked();
    assert!(!f.is_busy(cx));
}

#[gpui_kit::test]
fn pulls_the_checked_out_branch_as_the_pull_button_does(cx: &mut TestAppContext) {
    let f = render_with(cx, pulling);
    f.open_menu(MAIN, cx);
    f.choose("Pull main", cx);
    assert_eq!(f.fakes.pull.calls().len(), 1);
    assert!(f.fakes.pull_branch.calls().is_empty());
}

#[gpui_kit::test]
fn says_why_a_branch_couldnt_be_pulled(cx: &mut TestAppContext) {
    let f = render_with(cx, pulling);
    let message = "feature and origin/feature have diverged. Switch to feature to pull it.";
    f.fakes.pull_branch.fail_with(message);
    f.open_menu(FEATURE, cx);
    f.choose("Pull feature", cx);
    assert_eq!(f.branch_failure(cx), Some(("Pull", message.to_owned())));
}

#[gpui_kit::test]
fn isnt_offered_for_one_tracking_none_a_remote_one_or_one_checked_out_elsewhere(
    cx: &mut TestAppContext,
) {
    let f = render_with(cx, |id, cx| {
        pulling(id, cx);
        set_worktrees(id, worktrees(), cx);
    });
    for row in ["branch-refs/heads/lonely", ORIGIN_FEATURE, FEATURE] {
        let offered = f.open_menu(row, cx);
        assert!(offered.contains(&"Create branch…".to_owned()), "{row}");
        assert!(
            !offered.iter().any(|item| item.starts_with("Pull")),
            "{row}"
        );
        f.press("escape", cx);
    }
}

#[gpui_kit::test]
fn is_offered_for_the_branch_checked_out_in_the_worktree_on_show(cx: &mut TestAppContext) {
    let f = render_with(cx, |id, cx| {
        pulling(id, cx);
        set_worktrees(id, worktrees(), cx);
    });
    assert!(f.open_menu(MAIN, cx).contains(&"Pull main".to_owned()));
}

// ref-list.test.tsx: the stash menu.

fn with_stashes(id: &RepositoryId, cx: &mut TestAppContext) {
    set_stashes(id, vec![newer(), older()], cx);
}

#[gpui_kit::test]
fn pops_the_stash_right_clicked_an_older_one_too_showing_it_running_on_the_section(
    cx: &mut TestAppContext,
) {
    let f = render_with(cx, with_stashes);
    f.fakes.pop.hold.set(true);
    f.expand(Section::Stashes, cx);

    assert_eq!(
        f.open_menu(&stash_row(&older()), cx),
        items(&["Pop stash", "Delete stash…"])
    );
    f.choose("Pop stash", cx);
    assert_eq!(f.fakes.pop.calls(), [older().sha]);
    assert!(f.is_section_busy(Section::Stashes, cx));
    // The toolbar's Pop isn't the one running, but waits for it.
    let repository = f.repository(cx);
    assert!(repository.read_with(cx, |repository, _| {
        !repository
            .change_state(Change::Pop(PopFrom::Toolbar))
            .running
    }));
    assert!(repository.read_with(cx, |repository, _| repository.stashes_changing()));

    // Nor another one meanwhile.
    f.open_menu(&stash_row(&newer()), cx);
    f.choose("Pop stash", cx);
    f.open_menu(&stash_row(&newer()), cx);
    f.choose("Delete stash…", cx);
    assert_eq!(f.fakes.pop.calls().len(), 1);
    assert!(!f.has_dialog(cx));

    f.fakes.pop.finish(());
    cx.run_until_parked();
    assert!(!f.is_section_busy(Section::Stashes, cx));
}

#[gpui_kit::test]
fn says_beside_the_section_why_a_stash_couldnt_be_popped(cx: &mut TestAppContext) {
    let f = render_with(cx, with_stashes);
    let message = "Popping the stash caused conflicts.";
    f.fakes.pop.fail_with(message);
    f.expand(Section::Stashes, cx);

    f.open_menu(&stash_row(&older()), cx);
    f.choose("Pop stash", cx);
    assert_eq!(
        f.section_failure(Section::Stashes, cx),
        Some(("Pop stash".to_owned(), message.to_owned()))
    );
    // Only there, not under the toolbar's Pop as well.
    let repository = f.repository(cx);
    assert!(repository.read_with(cx, |repository, _| {
        repository
            .change_state(Change::Pop(PopFrom::Toolbar))
            .error
            .is_none()
    }));

    f.choose("Dismiss", cx);
    assert_eq!(f.section_failure(Section::Stashes, cx), None);
}

#[gpui_kit::test]
fn doesnt_delete_one_while_another_stash_action_runs(cx: &mut TestAppContext) {
    let f = render_with(cx, with_stashes);
    f.fakes.pop.hold.set(true);
    f.expand(Section::Stashes, cx);

    // The dialog was open before the pop began.
    f.open_menu(&stash_row(&older()), cx);
    f.choose("Delete stash…", cx);
    assert!(f.has_dialog(cx));
    // Begun straight on the actions: the dialog keeps the rest from being used, so this
    // stands for a pop begun some other way.
    f.repository(cx).update(cx, |repository, cx| {
        let sha = FullSha::parse(newer().sha).unwrap();
        drop(repository.pop_stash(PopFrom::Picked, sha, cx))
    });
    cx.run_until_parked();
    f.choose("Delete", cx);
    assert!(f.fakes.drop.calls().is_empty());

    f.fakes.pop.finish(());
}

#[gpui_kit::test]
fn forgets_the_stash_it_was_asking_about_deleting_in_another_repository(cx: &mut TestAppContext) {
    let f = render_with(cx, with_stashes);
    f.expand(Section::Stashes, cx);
    f.open_menu(&stash_row(&older()), cx);
    f.choose("Delete stash…", cx);
    assert!(f.has_dialog(cx));

    f.show(&f.other.clone(), cx);
    assert!(!f.has_dialog(cx));
    assert!(f.fakes.drop.calls().is_empty());
}

#[gpui_kit::test]
fn doesnt_pop_one_over_conflicts(cx: &mut TestAppContext) {
    let f = render_with(cx, |id, cx| {
        with_stashes(id, cx);
        let mut conflicted = status();
        conflicted.summary.counts.conflicted = 1;
        set_status(id, conflicted, cx);
    });
    f.expand(Section::Stashes, cx);
    f.open_menu(&stash_row(&newer()), cx);
    f.choose("Pop stash", cx);
    assert!(f.fakes.pop.calls().is_empty());
}

#[gpui_kit::test]
fn deletes_the_stash_right_clicked_once_thats_confirmed(cx: &mut TestAppContext) {
    let f = render_with(cx, with_stashes);
    f.expand(Section::Stashes, cx);

    f.open_menu(&stash_row(&older()), cx);
    f.choose("Delete stash…", cx);
    assert!(f.has_dialog(cx));
    f.choose("Cancel", cx);
    assert!(!f.has_dialog(cx));
    assert!(f.fakes.drop.calls().is_empty());

    f.open_menu(&stash_row(&older()), cx);
    f.choose("Delete stash…", cx);
    f.choose("Delete", cx);
    assert_eq!(f.fakes.drop.calls(), [older().sha]);
    assert!(f.fakes.pop.calls().is_empty());
}

#[gpui_kit::test]
fn says_why_a_stash_couldnt_be_deleted(cx: &mut TestAppContext) {
    let f = render_with(cx, with_stashes);
    let message = "The stash is gone: the stashes changed before it could be deleted.";
    f.fakes.drop.fail_with(message);
    f.expand(Section::Stashes, cx);

    f.open_menu(&stash_row(&newer()), cx);
    f.choose("Delete stash…", cx);
    f.choose("Delete", cx);
    assert_eq!(
        f.section_failure(Section::Stashes, cx),
        Some(("Delete stash".to_owned(), message.to_owned()))
    );
}

#[gpui_kit::test]
fn opens_no_branch_menu_for_a_stash(cx: &mut TestAppContext) {
    let f = render_with(cx, with_stashes);
    f.expand(Section::Stashes, cx);
    let offered = f.open_menu(&stash_row(&newer()), cx);
    assert!(offered.contains(&"Pop stash".to_owned()));
    assert!(!offered.contains(&"Create branch…".to_owned()));
    // And says which stash's it is.
    let list = f.list(cx);
    assert_eq!(
        list.read_with(cx, |list, _| list.stash_menu_for().map(ToString::to_string)),
        Some(newer().sha)
    );
}

// ref-list.test.tsx: the worktrees.

fn with_worktrees(id: &RepositoryId, cx: &mut TestAppContext) {
    set_worktrees(id, worktrees(), cx);
}

#[gpui_kit::test]
fn lists_them_the_one_on_show_marked_and_says_which_branches_are_checked_out_in_the_others(
    cx: &mut TestAppContext,
) {
    let f = render_with(cx, with_worktrees);
    for path in [
        "/w/repo",
        "/w/repo-feature",
        "/w/repo-check",
        "/w/repo-gone",
    ] {
        assert!(f.has_row(&worktree_row(path), cx), "{path}");
    }
    f.in_window(cx, |window, _| {
        let main = window.find(SharedString::from(worktree_row("/w/repo")));
        assert_eq!(main.label(), Some("repo"));
        assert_eq!(main.selected(), Some(true));
        assert_eq!(
            window.find("section-worktrees").label(),
            Some("Worktrees 4")
        );
        // A branch checked out in another worktree says so; the one checked out here
        // doesn't.
        assert!(
            window
                .within(SharedString::from(FEATURE))
                .try_find_by_label("Checked out in repo-feature")
                .is_some()
        );
        assert!(
            window
                .within(SharedString::from(MAIN))
                .find_all_by_role(gpui_kit::Role::Image)
                .is_empty()
        );
    });
}

#[gpui_kit::test]
fn opens_a_worktree_thats_double_clicked_as_a_repository_of_its_own_showing_it_running(
    cx: &mut TestAppContext,
) {
    let f = render_with(cx, with_worktrees);
    f.fakes.open.hold.set(true);

    f.double_click(&worktree_row("/w/repo-feature"), cx);
    assert_eq!(f.fakes.open.calls(), ["/w/repo-feature"]);
    assert!(f.is_section_busy(Section::Worktrees, cx));

    f.fakes.open.finish(());
    cx.run_until_parked();
    assert!(!f.is_section_busy(Section::Worktrees, cx));
    // The view that opened it shows it.
    assert_eq!(
        cx.update(|cx| Router::current(cx)),
        Route::Repository(worktree_id())
    );
}

#[gpui_kit::test]
fn doesnt_open_the_one_on_show_nor_one_whose_folder_is_gone(cx: &mut TestAppContext) {
    let f = render_with(cx, with_worktrees);
    f.double_click(&worktree_row("/w/repo"), cx);
    f.double_click(&worktree_row("/w/repo-gone"), cx);
    assert!(f.fakes.open.calls().is_empty());

    f.open_menu(&worktree_row("/w/repo"), cx);
    f.choose("Open worktree", cx);
    f.open_menu(&worktree_row("/w/repo-gone"), cx);
    f.choose("Open worktree", cx);
    assert!(f.fakes.open.calls().is_empty());
}

#[gpui_kit::test]
fn says_beside_the_section_why_a_worktree_couldnt_be_opened(cx: &mut TestAppContext) {
    let f = render_with(cx, with_worktrees);
    let message = "/w/repo-check is not a git repository.";
    f.fakes.open.fail_with(message);

    assert_eq!(
        f.open_menu(&worktree_row("/w/repo-check"), cx),
        items(&["Open worktree", "Remove worktree…"])
    );
    f.choose("Open worktree", cx);
    assert_eq!(f.fakes.open.calls(), ["/w/repo-check"]);
    assert_eq!(
        f.section_failure(Section::Worktrees, cx),
        Some(("Open worktree".to_owned(), message.to_owned()))
    );
}

#[gpui_kit::test]
fn removes_the_worktree_right_clicked_asking_first_showing_it_running_on_the_section(
    cx: &mut TestAppContext,
) {
    let f = render_with(cx, with_worktrees);
    f.fakes.remove.hold.set(true);

    f.open_menu(&worktree_row("/w/repo-feature"), cx);
    f.choose("Remove worktree…", cx);
    assert!(f.has_dialog(cx));
    assert!(f.fakes.remove.calls().is_empty());
    f.choose("Remove", cx);
    assert_eq!(f.fakes.remove.calls(), ["/w/repo-feature"]);
    assert!(f.is_section_busy(Section::Worktrees, cx));

    // Nor another one meanwhile.
    f.open_menu(&worktree_row("/w/repo-check"), cx);
    f.choose("Remove worktree…", cx);
    assert!(!f.has_dialog(cx));

    f.fakes.remove.finish(());
    cx.run_until_parked();
    assert!(!f.is_section_busy(Section::Worktrees, cx));
}

#[gpui_kit::test]
fn doesnt_remove_one_when_the_dialog_is_cancelled(cx: &mut TestAppContext) {
    let f = render_with(cx, with_worktrees);
    f.open_menu(&worktree_row("/w/repo-check"), cx);
    f.choose("Remove worktree…", cx);
    f.choose("Cancel", cx);
    assert!(!f.has_dialog(cx));
    assert!(f.fakes.remove.calls().is_empty());
}

#[gpui_kit::test]
fn doesnt_offer_removing_the_main_worktree_which_is_the_one_on_show(cx: &mut TestAppContext) {
    let f = render_with(cx, with_worktrees);
    f.open_menu(&worktree_row("/w/repo"), cx);
    f.choose("Remove worktree…", cx);
    assert!(!f.has_dialog(cx));
}

#[gpui_kit::test]
fn says_beside_the_section_why_a_worktree_couldnt_be_removed(cx: &mut TestAppContext) {
    let f = render_with(cx, with_worktrees);
    let message = "fatal: cannot remove a locked working tree, lock reason: busy";
    f.fakes.remove.fail_with(message);

    f.open_menu(&worktree_row("/w/repo-check"), cx);
    f.choose("Remove worktree…", cx);
    f.choose("Remove", cx);
    assert_eq!(
        f.section_failure(Section::Worktrees, cx),
        Some(("Remove worktree".to_owned(), message.to_owned()))
    );

    f.choose("Dismiss", cx);
    assert_eq!(f.section_failure(Section::Worktrees, cx), None);
}

#[gpui_kit::test]
fn forgets_the_worktree_it_was_asking_about_removing_in_another_repository(
    cx: &mut TestAppContext,
) {
    let f = render_with(cx, with_worktrees);
    f.open_menu(&worktree_row("/w/repo-check"), cx);
    f.choose("Remove worktree…", cx);
    assert!(f.has_dialog(cx));
    f.show(&f.other.clone(), cx);
    assert!(!f.has_dialog(cx));
    assert!(f.fakes.remove.calls().is_empty());
}

// ref-list.test.tsx: worktrees from the branch menu.

#[gpui_kit::test]
fn opens_the_worktree_a_branch_is_checked_out_in(cx: &mut TestAppContext) {
    let f = render_with(cx, with_worktrees);
    let offered = f.open_menu(FEATURE, cx);
    assert!(!offered.contains(&"Add worktree…".to_owned()));
    f.choose("Open worktree repo-feature", cx);
    assert_eq!(f.fakes.open.calls(), ["/w/repo-feature"]);
}

#[gpui_kit::test]
fn opens_the_worktree_a_branch_is_checked_out_in_when_its_double_clicked_rather_than_switching(
    cx: &mut TestAppContext,
) {
    let f = render_with(cx, with_worktrees);
    f.fakes.open.hold.set(true);

    f.double_click(FEATURE, cx);
    assert!(f.fakes.switch.calls().is_empty());
    assert_eq!(f.fakes.open.calls(), ["/w/repo-feature"]);
    assert!(f.is_section_busy(Section::Worktrees, cx));
    f.fakes.open.finish(());
}

#[gpui_kit::test]
fn says_beside_the_worktrees_why_one_couldnt_be_opened_from_a_branchs_menu(
    cx: &mut TestAppContext,
) {
    let f = render_with(cx, with_worktrees);
    let message = "/w/repo-feature is not a git repository.";
    f.fakes.open.fail_with(message);
    f.open_menu(FEATURE, cx);
    f.choose("Open worktree repo-feature", cx);
    assert_eq!(
        f.section_failure(Section::Worktrees, cx),
        Some(("Open worktree".to_owned(), message.to_owned()))
    );
}

#[gpui_kit::test]
fn adds_a_worktree_for_a_branch_suggesting_a_folder_beside_the_repository(cx: &mut TestAppContext) {
    let f = render_with(cx, with_worktrees);
    f.fakes.add.hold.set(true);

    f.open_menu(ORIGIN_FEATURE, cx);
    f.choose("Add worktree…", cx);
    assert!(f.has_dialog(cx));
    f.choose("Add worktree", cx);
    assert_eq!(
        f.fakes.add.calls(),
        [AddWorktreeInput::parse(
            "/w/repo-origin-feature",
            "refs/remotes/origin/feature",
            None
        )
        .unwrap()]
    );
    // The dialog closes, and the section shows it running.
    assert!(!f.has_dialog(cx));
    assert!(f.is_section_busy(Section::Worktrees, cx));

    f.fakes.add.finish(());
    cx.run_until_parked();
    assert!(!f.is_section_busy(Section::Worktrees, cx));
}

#[gpui_kit::test]
fn says_beside_the_section_why_a_worktree_couldnt_be_added_and_keeps_the_folder_to_fix(
    cx: &mut TestAppContext,
) {
    let f = render_with(cx, with_worktrees);
    let message = "fatal: '/w/repo-origin-feature' already exists";
    f.fakes.add.fail_with(message);

    f.open_menu(ORIGIN_FEATURE, cx);
    f.choose("Add worktree…", cx);
    f.choose("Add worktree", cx);
    assert_eq!(
        f.section_failure(Section::Worktrees, cx),
        Some(("Add worktree".to_owned(), message.to_owned()))
    );

    f.open_menu(ORIGIN_FEATURE, cx);
    f.choose("Add worktree…", cx);
    let provider = f.provider(cx);
    let dialog = provider.read_with(cx, |provider, _| provider.add_worktree_dialog().clone());
    let folder = dialog.read_with(cx, |dialog, _| dialog.folder().clone());
    assert_eq!(
        folder.read_with(cx, |folder, _| folder.value().to_string()),
        "/w/repo-origin-feature"
    );
    // Out of the way of the folder.
    assert_eq!(f.section_failure(Section::Worktrees, cx), None);
}

// ref-list.test.tsx: deleting from the branch menu.

#[gpui_kit::test]
fn deletes_a_local_branch_asking_first_showing_it_running(cx: &mut TestAppContext) {
    let f = render(cx);
    f.fakes.delete.hold.set(true);

    f.open_menu(FEATURE, cx);
    f.choose("Delete feature…", cx);
    assert!(f.has_dialog(cx));
    assert!(f.fakes.delete.calls().is_empty());
    f.choose("Delete", cx);
    assert_eq!(f.fakes.delete.calls(), ["refs/heads/feature"]);
    assert!(f.is_busy(cx));

    // Nor another one, or a merge, meanwhile.
    f.open_menu(ORIGIN_FEATURE, cx);
    f.choose("Delete origin/feature…", cx);
    assert!(!f.has_dialog(cx));
    f.open_menu(ORIGIN_FEATURE, cx);
    f.choose("Merge origin/feature into main", cx);
    assert!(f.fakes.merge.calls().is_empty());

    f.fakes.delete.finish(());
    cx.run_until_parked();
    assert!(!f.is_busy(cx));
}

#[gpui_kit::test]
fn doesnt_delete_it_when_the_dialog_is_cancelled(cx: &mut TestAppContext) {
    let f = render(cx);
    f.open_menu(FEATURE, cx);
    f.choose("Delete feature…", cx);
    f.choose("Cancel", cx);
    assert!(!f.has_dialog(cx));
    assert!(f.fakes.delete.calls().is_empty());
}

#[gpui_kit::test]
fn doesnt_switch_branches_nor_merge_while_it_runs(cx: &mut TestAppContext) {
    let f = render(cx);
    f.fakes.delete.hold.set(true);

    f.open_menu(ORIGIN_FEATURE, cx);
    f.choose("Delete origin/feature…", cx);
    f.choose("Delete", cx);
    assert!(f.is_busy(cx));
    f.double_click(FEATURE, cx);
    assert!(f.fakes.switch.calls().is_empty());

    f.fakes.delete.finish(());
    cx.run_until_parked();
    f.double_click(FEATURE, cx);
    assert_eq!(f.fakes.switch.calls(), ["refs/heads/feature"]);
}

#[gpui_kit::test]
fn doesnt_run_while_a_worktree_is_being_added_which_checks_out_a_branch_nor_that_while_it_runs(
    cx: &mut TestAppContext,
) {
    let f = render(cx);
    f.fakes.add.hold.set(true);
    f.fakes.delete.hold.set(true);

    f.open_menu(FEATURE, cx);
    f.choose("Add worktree…", cx);
    f.choose("Add worktree", cx);
    assert!(f.is_section_busy(Section::Worktrees, cx));
    f.open_menu(ORIGIN_FEATURE, cx);
    f.choose("Delete origin/feature…", cx);
    assert!(!f.has_dialog(cx));

    f.fakes.add.finish(());
    cx.run_until_parked();
    assert!(!f.is_section_busy(Section::Worktrees, cx));
    f.open_menu(ORIGIN_FEATURE, cx);
    f.choose("Delete origin/feature…", cx);
    f.choose("Delete", cx);
    assert!(f.is_busy(cx));
    f.open_menu(FEATURE, cx);
    f.choose("Add worktree…", cx);
    assert!(!f.has_dialog(cx));

    f.fakes.delete.finish(());
}

#[gpui_kit::test]
fn isnt_offered_for_a_branch_checked_out_here_or_in_another_worktree_nor_a_remote_one_tracked_by_it(
    cx: &mut TestAppContext,
) {
    let f = render_with(cx, |id, cx| {
        with_worktrees(id, cx);
        set_refs(
            id,
            vec![
                tracking("main", true),
                tracking("feature", false),
                local("topic", false),
                remote("origin/main"),
                remote("origin/feature"),
            ],
            cx,
        );
    });
    let provider = f.provider(cx);
    provider.read_with(cx, |provider, cx| {
        assert!(provider.deletion("refs/heads/topic", cx).is_some());
        for reference in [
            "refs/heads/main",
            "refs/heads/feature",
            "refs/remotes/origin/main",
            "refs/remotes/origin/feature",
        ] {
            assert!(provider.deletion(reference, cx).is_none(), "{reference}");
        }
    });
    f.open_menu(FEATURE, cx);
    f.choose("Delete feature…", cx);
    assert!(!f.has_dialog(cx));
}

#[gpui_kit::test]
fn says_why_a_branch_couldnt_be_deleted_until_thats_dismissed(cx: &mut TestAppContext) {
    let f = render(cx);
    let message = "! [remote rejected] feature (protected branch hook declined)";
    f.fakes.delete.fail_with(message);

    f.open_menu(ORIGIN_FEATURE, cx);
    f.choose("Delete origin/feature…", cx);
    f.choose("Delete", cx);
    assert_eq!(
        f.branch_failure(cx),
        Some(("Delete branch", message.to_owned()))
    );

    f.choose("Dismiss", cx);
    assert_eq!(f.branch_failure(cx), None);
}

/// The id of the row the keyboard's on.
fn cursor(f: &Fixture, cx: &mut TestAppContext) -> Option<String> {
    let list = f.list(cx);
    list.read_with(cx, |list, _| {
        list.cursor.as_ref().map(|row| row.element_id().to_string())
    })
}

fn is_outlined(f: &Fixture, row: &str, cx: &mut TestAppContext) -> bool {
    let row = SharedString::from(row.to_owned());
    f.in_window(cx, |window, cx| {
        window.render_frame(cx);
        window.within(row).try_find("keyboard-outline").is_some()
    })
}

#[gpui_kit::test]
fn collapses_a_section_from_its_header_with_enter_or_space(cx: &mut TestAppContext) {
    let f = render(cx);
    // Clicking the header collapses it, and gives it the focus.
    f.click("section-local", cx);
    assert!(!f.has_row(MAIN, cx));
    f.press("enter", cx);
    assert!(f.has_row(MAIN, cx));
    f.press("space", cx);
    assert!(!f.has_row(MAIN, cx));
    // The arrows are the list's, not its headers'.
    f.press("down", cx);
    assert_eq!(cursor(&f, cx), None);
}

#[gpui_kit::test]
fn moves_through_the_expanded_sections_rows_with_the_arrows_home_and_end(cx: &mut TestAppContext) {
    let f = render(cx);
    f.click(MAIN, cx);
    assert_eq!(cursor(&f, cx).as_deref(), Some(MAIN));
    // The pointer put it there: it isn't outlined.
    assert!(!is_outlined(&f, MAIN, cx));

    // The last row: the remote branch, as the tags and the stashes are collapsed.
    f.press("end", cx);
    assert_eq!(cursor(&f, cx).as_deref(), Some(ORIGIN_FEATURE));
    assert!(is_outlined(&f, ORIGIN_FEATURE, cx));
    f.press("down", cx);
    assert_eq!(cursor(&f, cx).as_deref(), Some(ORIGIN_FEATURE));
    // Across into the remotes' folder, then the local branches.
    f.press("up", cx);
    assert_eq!(
        cursor(&f, cx).as_deref(),
        Some("folder-refs/remotes/origin")
    );
    f.press("up", cx);
    assert!(cursor(&f, cx).unwrap().starts_with("branch-refs/heads/"));
    f.press("home", cx);
    let first = cursor(&f, cx).unwrap();
    assert!(first.starts_with("branch-refs/heads/"));
    f.press("up", cx);
    assert_eq!(cursor(&f, cx), Some(first));
    assert!(f.fakes.switch.calls().is_empty());
}

#[gpui_kit::test]
fn switches_to_the_branch_the_keyboards_on_with_enter(cx: &mut TestAppContext) {
    let f = render(cx);
    f.click(MAIN, cx);
    f.press("end", cx);
    f.press("enter", cx);
    assert_eq!(
        f.fakes.switch.calls(),
        vec!["refs/remotes/origin/feature".to_owned()]
    );
}

#[gpui_kit::test]
fn toggles_the_folder_the_keyboards_on_with_enter(cx: &mut TestAppContext) {
    let f = render(cx);
    f.click(MAIN, cx);
    f.press("end", cx);
    f.press("up", cx);
    f.press("enter", cx);
    assert!(!f.has_row(ORIGIN_FEATURE, cx));
    assert_eq!(
        cursor(&f, cx).as_deref(),
        Some("folder-refs/remotes/origin")
    );
    f.press("enter", cx);
    assert!(f.has_row(ORIGIN_FEATURE, cx));
}

#[gpui_kit::test]
fn opens_the_menu_of_the_branch_the_keyboards_on_with_shift_f10(cx: &mut TestAppContext) {
    let f = render(cx);
    f.click(FEATURE, cx);
    f.press("shift-f10", cx);
    let items = f.in_window(cx, |window, _| {
        window
            .within("popup-menu")
            .find_all_by_role(gpui_kit::Role::MenuItem)
            .into_iter()
            .map(|item| item.label().unwrap_or_default().to_owned())
            .collect::<Vec<_>>()
    });
    assert!(items.contains(&"Create branch…".to_owned()));
    let provider = f.provider(cx);
    assert_eq!(
        provider.read_with(cx, |provider, _| provider.open_for().cloned()),
        Some(SharedString::from("refs/heads/feature"))
    );

    f.press("escape", cx);
    assert!(!f.is_menu_open(cx));
    // The list has the keyboard back.
    f.press("down", cx);
    assert_ne!(cursor(&f, cx).as_deref(), Some(FEATURE));

    // A folder has no menu.
    f.press("end", cx);
    f.press("up", cx);
    f.press("shift-f10", cx);
    assert!(!f.is_menu_open(cx));
}

#[gpui_kit::test]
fn opens_the_menus_of_the_branch_menus_its_given_rather_than_the_last_made(
    cx: &mut TestAppContext,
) {
    let f = render(cx);
    // Another page's, made after this one's.
    let other = f.other.clone();
    let _later = f.in_window(cx, |window, cx| {
        cx.new(|cx| BranchMenuProvider::new(other, window, cx))
    });
    assert!(
        f.open_menu(FEATURE, cx)
            .contains(&"Create branch…".to_owned())
    );
    let provider = f.provider(cx);
    assert_eq!(
        provider.read_with(cx, |provider, _| provider.open_for().cloned()),
        Some(SharedString::from("refs/heads/feature"))
    );
}
