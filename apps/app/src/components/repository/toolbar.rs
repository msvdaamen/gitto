//! The repository's toolbar. Ported from `components/repository/toolbar.tsx`.

use gitto_git::features::status::schema::{StatusSummary, Uncommitted};
use gitto_git::input::RepositoryId;
use gitto_repository::Repository;
use gitto_ui::git::status::head_label;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Context, Entity, EventEmitter, FontWeight, SharedString, Subscription, Window, div, rems,
};

use super::branch_button::BranchButton;
use super::pull_button::PullButton;
use super::push_button::PushButton;
use super::stash_buttons::StashButtons;
use super::toolbar_button::{HideBelow, ToolbarButton};
use crate::components::ui::{Divider, FailurePopover, Icon, IconButton, IconName, TextInput};
use crate::data::branch::{
    self, delete_branch_state, merge_branch_state, rebase_branch_state, switch_branch_state,
};
use crate::data::remote::{self, fetch_operation, pull_branch_state};
use crate::data::repositories::{find, repositories_query};
use crate::data::status::status_query;
use crate::data::{Opaque, OperationState, Operations, Query, QueryError, RepositoryOperation};
use crate::theme::PaletteExt as _;
use crate::theme::breakpoints::{MEDIUM, SMALL};

/// The toolbar's height (TS's grid row, 49px).
pub const TOOLBAR_HEIGHT: f32 = 3.25;

/// What the toolbar asks of the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolbarEvent {
    ToggleSidebar,
    ToggleDetails,
    /// The history's search changed.
    Search(SharedString),
}

/// One of the changes of the branches the current branch shows, by what it's called in its
/// failure's title.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchChangeKind {
    Merge,
    Rebase,
    Switch,
    Delete,
    Pull,
}

impl BranchChangeKind {
    const ALL: [BranchChangeKind; 5] = [
        BranchChangeKind::Merge,
        BranchChangeKind::Rebase,
        BranchChangeKind::Switch,
        BranchChangeKind::Delete,
        BranchChangeKind::Pull,
    ];

    /// What its failure is titled.
    pub fn title(self) -> &'static str {
        match self {
            BranchChangeKind::Merge => "Merge",
            BranchChangeKind::Rebase => "Rebase",
            BranchChangeKind::Switch => "Switch branch",
            BranchChangeKind::Delete => "Delete branch",
            BranchChangeKind::Pull => "Pull",
        }
    }

    fn state(self, id: &str, cx: &App) -> OperationState {
        match self {
            BranchChangeKind::Merge => merge_branch_state(id, cx),
            BranchChangeKind::Rebase => rebase_branch_state(id, cx),
            BranchChangeKind::Switch => switch_branch_state(id, cx),
            BranchChangeKind::Delete => delete_branch_state(id, cx),
            BranchChangeKind::Pull => pull_branch_state(id, cx),
        }
    }

    /// The name of its operation (see `data::branch` and `data::remote`).
    fn operation(self) -> &'static str {
        match self {
            BranchChangeKind::Merge => branch::MERGE,
            BranchChangeKind::Rebase => branch::REBASE,
            BranchChangeKind::Switch => branch::SWITCH,
            BranchChangeKind::Delete => branch::DELETE,
            BranchChangeKind::Pull => remote::PULL_BRANCH,
        }
    }
}

/// Which of the changes of the branches failed, the last first: each can have failed, one after
/// the other, and they're shown one at a time. Told each one's error as it is, it puts one that
/// newly failed first.
#[derive(Debug, Clone, PartialEq)]
pub struct FailureOrder {
    order: Vec<BranchChangeKind>,
    errors: Vec<(BranchChangeKind, Option<QueryError>)>,
}

impl Default for FailureOrder {
    fn default() -> Self {
        FailureOrder {
            order: BranchChangeKind::ALL.to_vec(),
            errors: BranchChangeKind::ALL
                .iter()
                .map(|kind| (*kind, None))
                .collect(),
        }
    }
}

impl FailureOrder {
    /// Takes in each change's error now (`error_of`), putting first one that failed since.
    pub fn update(&mut self, error_of: impl Fn(BranchChangeKind) -> Option<QueryError>) {
        for (kind, last) in &mut self.errors {
            let error = error_of(*kind);
            if error != *last {
                if error.is_some() {
                    self.order.retain(|other| other != kind);
                    self.order.insert(0, *kind);
                }
                *last = error;
            }
        }
    }

    /// The failure to show: the last to fail of those that still have an error.
    pub fn shown(&self) -> Option<(BranchChangeKind, QueryError)> {
        self.order.iter().find_map(|kind| {
            self.errors
                .iter()
                .find(|(other, _)| other == kind)
                .and_then(|(_, error)| error.clone())
                .map(|error| (*kind, error))
        })
    }
}

/// The repository's toolbar: the sidebar's toggle and the current branch, undo and redo, Pull,
/// Push, Branch, the stash's buttons and Fetch, then the history's search and the details'
/// toggle. An entity: it keeps the search box and the buttons. The page tells it which panels are
/// open ([`set_panels`](Self::set_panels)) and handles its [`ToolbarEvent`]s.
///
/// The current branch shows the repository's name and where HEAD is, with a spinner in place of
/// the branch icon while switching branches, merging one into it, rebasing it onto one, or
/// deleting or pulling another (from a branch's menu), and a popover under it says why that
/// failed.
pub struct RepositoryToolbar {
    repository_id: RepositoryId,
    search: Entity<InputState>,
    sidebar_open: bool,
    details_open: bool,
    status: Query<Opaque<Uncommitted>, StatusSummary>,
    repositories: Query<Vec<Repository>>,
    /// Fetching every remote of the repository.
    fetch: RepositoryOperation<()>,
    failures: FailureOrder,
    pull: Entity<PullButton>,
    push: Entity<PushButton>,
    branch: Entity<BranchButton>,
    stash: Entity<StashButtons>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ToolbarEvent> for RepositoryToolbar {}

impl RepositoryToolbar {
    pub fn new(repository_id: RepositoryId, window: &mut Window, cx: &mut Context<Self>) -> Self {
        RepositoryToolbar::with_fetch(repository_id, fetch_operation(), window, cx)
    }

    /// With `fetch` fetching: a test's.
    pub(crate) fn with_fetch(
        repository_id: RepositoryId,
        fetch: RepositoryOperation<()>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search history…"));
        let subscriptions = vec![
            cx.subscribe(&search, |_, search, event, cx| {
                if matches!(event, InputEvent::Change) {
                    let value = search.read(cx).value();
                    cx.emit(ToolbarEvent::Search(value));
                }
            }),
            cx.observe_global::<Operations>(|this, cx| {
                this.update_failures(cx);
                cx.notify();
            }),
        ];
        let id = repository_id.clone();
        let mut toolbar = RepositoryToolbar {
            pull: cx.new(|cx| PullButton::new(id.clone(), window, cx)),
            push: cx.new(|cx| PushButton::new(id.clone(), window, cx)),
            branch: cx.new(|cx| BranchButton::new(id.clone(), window, cx)),
            stash: cx.new(|cx| StashButtons::new(id, window, cx)),
            status: status_query(&repository_id, cx),
            repositories: repositories_query(cx),
            repository_id,
            search,
            sidebar_open: true,
            details_open: true,
            fetch,
            failures: FailureOrder::default(),
            _subscriptions: subscriptions,
        };
        toolbar.update_failures(cx);
        toolbar
    }

    pub fn repository_id(&self) -> &RepositoryId {
        &self.repository_id
    }

    /// Puts `search` in the search box: the history's search, as the toolbar of the repository
    /// on show before had it.
    pub fn set_search(&mut self, search: &str, window: &mut Window, cx: &mut Context<Self>) {
        let search = SharedString::from(search.to_owned());
        self.search
            .update(cx, |input, cx| input.set_value(search, window, cx));
    }

    /// Which panels are open, for the toggles to show.
    pub fn set_panels(&mut self, sidebar_open: bool, details_open: bool, cx: &mut Context<Self>) {
        if (self.sidebar_open, self.details_open) != (sidebar_open, details_open) {
            self.sidebar_open = sidebar_open;
            self.details_open = details_open;
            cx.notify();
        }
    }

    fn update_failures(&mut self, cx: &App) {
        let id = self.repository_id.clone();
        self.failures.update(|kind| kind.state(&id, cx).error);
    }

    /// Whether the current branch is changing (see the type's doc).
    pub fn is_branch_busy(&self, cx: &App) -> bool {
        BranchChangeKind::ALL
            .iter()
            .any(|kind| kind.state(&self.repository_id, cx).running)
    }

    /// The failure the current branch shows, if any.
    pub fn branch_failure(&self) -> Option<(BranchChangeKind, QueryError)> {
        self.failures.shown()
    }

    /// Fetch, as it is now: spinning while it runs, red with the reason once it failed. Both
    /// are the repository's: a fetch of another stays with that one.
    pub fn fetch_button(&self, cx: &App) -> ToolbarButton {
        let state = self.fetch.state(&self.repository_id, cx);
        let title = match &state.error {
            Some(error) => format!("Fetch failed: {}", error.message()),
            None => "Fetch all remotes".to_owned(),
        };
        let fetch = self.fetch.clone();
        let id = self.repository_id.clone();
        ToolbarButton::new("fetch", IconName::RefreshCw, "Fetch")
            .hide_below(HideBelow::Small)
            .title(title)
            .disabled(state.running)
            .danger(state.error.is_some())
            .busy(state.running)
            .on_click(move |_, _, cx| drop(fetch.run(&id, (), cx)))
    }

    fn render_current_branch(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = *cx.palette();
        let busy = self.is_branch_busy(cx);
        let name = self
            .repositories
            .data()
            .and_then(|list| find(&list, &self.repository_id).map(|r| r.name.clone()))
            .unwrap_or_default();
        let head = self
            .status
            .data()
            .map(|status| head_label(&status.head).to_owned())
            .unwrap_or_default();
        let failure = self.branch_failure();
        let (title, error) = match &failure {
            Some((kind, error)) => (kind.title(), Some(error.clone())),
            None => ("Merge", None),
        };
        let dismiss = failure.map(|(kind, _)| kind);
        let id = self.repository_id.clone();
        let with_name = f32::from(window.viewport_size().width) >= MEDIUM;
        let mark = div()
            .flex()
            .flex_none()
            .size(rems(1.8))
            .items_center()
            .justify_center()
            .rounded_md()
            .bg(palette.primary_soft)
            .text_color(palette.primary_strong)
            .child(if busy {
                Spinner::new()
                    .icon(Icon::new(IconName::LoaderCircle))
                    .color(palette.primary_strong)
                    .small()
                    .into_any_element()
            } else {
                Icon::new(IconName::GitBranch).size_4().into_any_element()
            });
        let trigger = CurrentBranch {
            mark: mark.into_any_element(),
            name: with_name.then(|| (SharedString::from(name), SharedString::from(head))),
            busy,
            selected: false,
        };
        FailurePopover::new("current-branch-failure", title, error, trigger).on_dismiss(
            move |_, cx| {
                if let Some(kind) = dismiss {
                    Operations::dismiss(kind.operation(), &id, cx);
                }
            },
        )
    }
}

/// The current branch's mark, the repository's name and where HEAD is: what the failure popover
/// of the changes of the branches is under.
#[derive(IntoElement)]
struct CurrentBranch {
    mark: gpui_kit::AnyElement,
    /// The repository's name, and where HEAD is; left out below the `md` breakpoint.
    name: Option<(SharedString, SharedString)>,
    busy: bool,
    selected: bool,
}

impl gpui_kit::component::Selectable for CurrentBranch {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.selected
    }
}

impl RenderOnce for CurrentBranch {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        h_flex()
            .id("current-branch")
            .min_w_0()
            .gap_2()
            .aria_label(if self.busy {
                "Current branch (changing)"
            } else {
                "Current branch"
            })
            .child(self.mark)
            .when_some(self.name, |this, (name, head)| {
                this.child(
                    v_flex()
                        .min_w_0()
                        .child(
                            div()
                                .truncate()
                                .text_size(rems(0.9))
                                .font_weight(FontWeight::BOLD)
                                .text_color(palette.text)
                                .child(name),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(rems(0.73))
                                .text_color(palette.faint)
                                .child(head),
                        ),
                )
            })
    }
}

impl Render for RepositoryToolbar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = *cx.palette();
        let width = f32::from(window.viewport_size().width);
        let small = width < SMALL;
        let medium = width >= MEDIUM;
        h_flex()
            .h(rems(TOOLBAR_HEIGHT))
            .flex_none()
            .min_w_0()
            .when(small, |this| this.gap_0p5())
            .when(!small, |this| this.gap_2())
            .px_2()
            .border_b_1()
            .border_color(palette.border)
            .bg(palette.panel)
            .child(
                h_flex()
                    .min_w_0()
                    .gap_2()
                    .when(medium, |this| this.min_w(rems(13.)))
                    .child(
                        IconButton::new("toggle-sidebar", IconName::PanelLeft, "Toggle sidebar")
                            .active(self.sidebar_open)
                            .on_click(
                                cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::ToggleSidebar)),
                            ),
                    )
                    .child(self.render_current_branch(window, cx)),
            )
            .child(
                h_flex()
                    .flex_none()
                    .gap_0p5()
                    .children(undo_and_redo())
                    .child(Divider::new().tall())
                    .child(self.pull.clone())
                    .child(self.push.clone())
                    .child(self.branch.clone())
                    .child(self.stash.clone())
                    .child(self.fetch_button(cx)),
            )
            .child(
                div()
                    .ml_auto()
                    .min_w_0()
                    .child(TextInput::new(&self.search).compact(true)),
            )
            .child(
                IconButton::new(
                    "toggle-details",
                    IconName::PanelRight,
                    "Toggle details panel",
                )
                .active(self.details_open)
                .on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::ToggleDetails))),
            )
    }
}

/// Undo and Redo, which don't do anything yet: disabled, saying so.
fn undo_and_redo() -> [ToolbarButton; 2] {
    [
        ToolbarButton::new("undo", IconName::Undo2, "Undo").title("Undo (coming soon)"),
        ToolbarButton::new("redo", IconName::Redo2, "Redo").title("Redo (coming soon)"),
    ]
    .map(|button| button.hide_below(HideBelow::Large).disabled(true))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use futures::channel::oneshot;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AnyWindowHandle, AppContext as _, TestAppContext};

    use super::*;
    use crate::components::repository::test_support::{
        Repositories, click_label, repository, repository_id, set_operation, set_push_target,
        set_refs, set_remotes, set_stashes, set_status, set_worktrees, setup_with, status,
    };

    /// Gives `id` what the toolbar's buttons show, so none of them loads from git.
    fn seed(id: &RepositoryId, cx: &mut TestAppContext) {
        set_status(id, status(), cx);
        set_stashes(id, vec![], cx);
        set_push_target(
            id,
            gitto_git::features::remote::schema::PushTarget::Blocked {
                reason: "There's no remote to push to.".into(),
            },
            cx,
        );
        set_operation(id, None, cx);
        set_refs(id, vec![], cx);
        set_worktrees(id, vec![], cx);
        set_remotes(id, &[], cx);
    }

    /// The fetches asked for, and what settles the one under way.
    #[derive(Default)]
    struct Fetches {
        calls: RefCell<Vec<RepositoryId>>,
        settle: RefCell<Option<oneshot::Sender<Result<(), QueryError>>>>,
    }

    fn render(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        RepositoryId,
        RepositoryId,
        Rc<Fetches>,
        AnyWindowHandle,
        Entity<RepositoryToolbar>,
    ) {
        let a = repository_id();
        let b = repository_id();
        let repositories = Rc::new(Repositories::default());
        repositories
            .list
            .borrow_mut()
            .extend([repository(&a, "/work/a"), repository(&b, "/work/b")]);
        let dir = setup_with(cx, repositories);
        seed(&a, cx);
        seed(&b, cx);
        let fetches = Rc::new(Fetches::default());
        let (window, toolbar) = switch(&a, &fetches, cx);
        (dir, a, b, fetches, window, toolbar)
    }

    /// The toolbar of the repository `id`, fetching into `fetches`, as the page makes it when
    /// that repository comes on show.
    fn switch(
        id: &RepositoryId,
        fetches: &Rc<Fetches>,
        cx: &mut TestAppContext,
    ) -> (AnyWindowHandle, Entity<RepositoryToolbar>) {
        let shown = id.clone();
        let fetches = Rc::clone(fetches);
        let opened = crate::test_support::open(cx, move |window, cx| {
            let fetch =
                RepositoryOperation::new("fetch", move |id: RepositoryId, (), cx: &mut App| {
                    fetches.calls.borrow_mut().push(id);
                    let (settle, settled) = oneshot::channel();
                    *fetches.settle.borrow_mut() = Some(settle);
                    cx.background_spawn(async move {
                        settled.await.unwrap_or(Err(QueryError::msg("dropped")))
                    })
                });
            RepositoryToolbar::with_fetch(shown, fetch, window, cx)
        });
        cx.run_until_parked();
        opened
    }

    fn fetch_button(toolbar: &Entity<RepositoryToolbar>, cx: &mut TestAppContext) -> ToolbarButton {
        toolbar.read_with(cx, |toolbar, cx| toolbar.fetch_button(cx))
    }

    #[gpui_kit::test]
    fn fetches_the_repository_and_shows_that_its_running_and_why_it_failed(
        cx: &mut TestAppContext,
    ) {
        let (_dir, a, b, fetches, window, toolbar) = render(cx);

        cx.update_window(window, |_, window, cx| window.click("fetch", cx))
            .unwrap();
        cx.run_until_parked();
        assert_eq!(*fetches.calls.borrow(), std::slice::from_ref(&a));
        let shown = fetch_button(&toolbar, cx);
        // Busy, not unavailable: it isn't faded.
        assert!(shown.is_busy());

        // Another repository's fetch is its own.
        let (_, other) = switch(&b, &fetches, cx);
        assert!(!fetch_button(&other, cx).is_busy());
        let (window, toolbar) = switch(&a, &fetches, cx);
        assert!(fetch_button(&toolbar, cx).is_busy());

        let settle = fetches.settle.borrow_mut().take().unwrap();
        settle
            .send(Err(QueryError::msg("Could not resolve host: example.com")))
            .unwrap();
        cx.run_until_parked();
        let shown = fetch_button(&toolbar, cx);
        assert!(!shown.is_busy());
        assert_eq!(
            shown.tooltip(),
            "Fetch failed: Could not resolve host: example.com"
        );

        cx.update_window(window, |_, window, cx| window.click("fetch", cx))
            .unwrap();
        cx.run_until_parked();
        assert_eq!(fetch_button(&toolbar, cx).tooltip(), "Fetch all remotes");
        assert_eq!(fetches.calls.borrow().len(), 2);
        let settle = fetches.settle.borrow_mut().take().unwrap();
        settle.send(Ok(())).unwrap();
        cx.run_until_parked();
        let shown = fetch_button(&toolbar, cx);
        assert!(!shown.is_busy());
        assert_eq!(shown.tooltip(), "Fetch all remotes");
    }

    #[gpui_kit::test]
    fn keeps_a_repositorys_fetch_failure_while_another_is_fetched(cx: &mut TestAppContext) {
        let (_dir, a, b, fetches, window, _) = render(cx);
        let settle = |result: Result<(), QueryError>, cx: &mut TestAppContext| {
            let settle = fetches.settle.borrow_mut().take().unwrap();
            settle.send(result).unwrap();
            cx.run_until_parked();
        };

        cx.update_window(window, |_, window, cx| window.click("fetch", cx))
            .unwrap();
        settle(Err(QueryError::msg("Could not resolve host.")), cx);
        let (window, toolbar) = switch(&b, &fetches, cx);
        assert_eq!(fetch_button(&toolbar, cx).tooltip(), "Fetch all remotes");
        cx.update_window(window, |_, window, cx| window.click("fetch", cx))
            .unwrap();
        settle(Ok(()), cx);
        assert_eq!(*fetches.calls.borrow(), [a.clone(), b.clone()]);

        let (_, toolbar) = switch(&a, &fetches, cx);
        assert_eq!(
            fetch_button(&toolbar, cx).tooltip(),
            "Fetch failed: Could not resolve host."
        );
    }

    #[gpui_kit::test]
    fn shows_the_last_change_of_the_branches_to_fail(cx: &mut TestAppContext) {
        let (_dir, a, _, _, _, toolbar) = render(cx);
        let fail = |name: &'static str, message: &'static str, cx: &mut TestAppContext| {
            let operation = RepositoryOperation::new(name, move |_, (), cx: &mut App| {
                cx.background_spawn(async move { Err::<(), _>(QueryError::msg(message)) })
            });
            let a = a.clone();
            cx.update(|cx| drop(operation.run(&a, (), cx)));
            cx.run_until_parked();
        };

        fail("merge", "Merge conflict.", cx);
        fail("delete-branch", "Not fully merged.", cx);
        let shown = toolbar.read_with(cx, |toolbar, _| toolbar.branch_failure());
        let (kind, error) = shown.unwrap();
        assert_eq!(kind.title(), "Delete branch");
        assert_eq!(error.message(), "Not fully merged.");

        // Dismissed, the one before it shows again.
        cx.update(|cx| Operations::dismiss("delete-branch", &a, cx));
        let (kind, _) = toolbar
            .read_with(cx, |toolbar, _| toolbar.branch_failure())
            .unwrap();
        assert_eq!(kind.title(), "Merge");
    }

    #[gpui_kit::test]
    fn dismisses_the_failure_shown_from_its_popover(cx: &mut TestAppContext) {
        let (_dir, a, _, _, window, toolbar) = render(cx);
        let merge = RepositoryOperation::new("merge", |_, (), cx: &mut App| {
            cx.background_spawn(async { Err::<(), _>(QueryError::msg("Merge conflict.")) })
        });
        cx.update(|cx| drop(merge.run(&a, (), cx)));
        cx.run_until_parked();
        assert!(
            toolbar
                .read_with(cx, |toolbar, _| toolbar.branch_failure())
                .is_some()
        );

        click_label(window, "Dismiss", cx);
        assert!(
            toolbar
                .read_with(cx, |toolbar, _| toolbar.branch_failure())
                .is_none()
        );
        assert!(cx.update(|cx| merge.state(&a, cx)).error.is_none());
    }

    #[test]
    fn says_undo_and_redo_are_still_to_come() {
        for button in undo_and_redo() {
            assert!(button.is_disabled());
            assert_eq!(
                button.tooltip().as_ref(),
                format!("{} (coming soon)", button.label())
            );
        }
    }

    #[test]
    fn puts_the_change_that_failed_last_first() {
        let diverged = QueryError::msg("Diverged.");
        let local_changes = QueryError::msg("Local changes.");
        let mut order = FailureOrder::default();
        assert!(order.shown().is_none());
        order.update(|kind| (kind == BranchChangeKind::Pull).then(|| diverged.clone()));
        order.update(|kind| match kind {
            BranchChangeKind::Pull => Some(diverged.clone()),
            BranchChangeKind::Switch => Some(local_changes.clone()),
            _ => None,
        });
        assert_eq!(order.shown().unwrap().0, BranchChangeKind::Switch);
        // Dismissed, the one before it shows again.
        order.update(|kind| (kind == BranchChangeKind::Pull).then(|| diverged.clone()));
        assert_eq!(order.shown().unwrap().0, BranchChangeKind::Pull);
    }
}
