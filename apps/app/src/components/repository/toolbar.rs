//! The repository's toolbar. Ported from `components/repository/toolbar.tsx`.

use gitto_git::input::RepositoryId;
use gitto_ui_logic::git::status::head_label;
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
use super::toolbar_button::{HideBelow, SMALL, ToolbarButton};
use crate::views::repository::MEDIUM;
use gitto_app_data::data::repositories::find;
use gitto_app_data::data::{AppData, AppPart, Change, Changed, DataError, Part, Repository, Shown};
use gitto_app_kit::theme::PaletteExt as _;
use gitto_app_kit::ui::{Divider, FailurePopover, Icon, IconButton, IconName, TextInput};

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

    /// The repository's change it is.
    pub fn change(self) -> Change {
        match self {
            BranchChangeKind::Merge => Change::Merge,
            BranchChangeKind::Rebase => Change::Rebase,
            BranchChangeKind::Switch => Change::Switch,
            BranchChangeKind::Delete => Change::DeleteBranch,
            BranchChangeKind::Pull => Change::PullBranch,
        }
    }
}

/// Which of the changes of the branches failed, the last first: each can have failed, one after
/// the other, and they're shown one at a time. Told each one's error as it is, it puts one that
/// newly failed first.
#[derive(Debug, Clone, PartialEq)]
pub struct FailureOrder {
    order: Vec<BranchChangeKind>,
    errors: Vec<(BranchChangeKind, Option<DataError>)>,
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
    pub fn update(&mut self, error_of: impl Fn(BranchChangeKind) -> Option<DataError>) {
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
    pub fn shown(&self) -> Option<(BranchChangeKind, DataError)> {
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
    repository: Shown,
    search: Entity<InputState>,
    sidebar_open: bool,
    details_open: bool,
    /// Takes in the changes of the branches' failures as they change.
    _failures: Subscription,
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
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search history…"));
        let subscriptions = vec![
            cx.subscribe(&search, |_, search, event, cx| {
                if matches!(event, InputEvent::Change) {
                    let value = search.read(cx).value();
                    cx.emit(ToolbarEvent::Search(value));
                }
            }),
            cx.subscribe(&AppData::global(cx), |_, _, event, cx| {
                if event.0 == AppPart::Repositories {
                    cx.notify();
                }
            }),
        ];
        AppData::global(cx).update(cx, |data, cx| data.show(AppPart::Repositories, cx));
        let repository = Shown::new(&repository_id, &[Part::Status, Part::Changes], cx);
        let id = repository_id.clone();
        let mut toolbar = RepositoryToolbar {
            _failures: Self::follow_failures(repository.entity(), cx),
            repository,
            pull: cx.new(|cx| PullButton::new(id.clone(), window, cx)),
            push: cx.new(|cx| PushButton::new(id.clone(), window, cx)),
            branch: cx.new(|cx| BranchButton::new(id.clone(), window, cx)),
            stash: cx.new(|cx| StashButtons::new(id, window, cx)),
            search,
            sidebar_open: true,
            details_open: true,
            failures: FailureOrder::default(),
            _subscriptions: subscriptions,
        };
        toolbar.update_failures(cx);
        toolbar
    }

    fn follow_failures(repository: &Entity<Repository>, cx: &mut Context<Self>) -> Subscription {
        cx.subscribe(repository, |this, _, Changed(part), cx| {
            if *part == Part::Changes {
                this.update_failures(cx);
            }
        })
    }

    /// The id of the repository on show.
    pub fn repository_id<'a>(&self, cx: &'a App) -> &'a RepositoryId {
        self.repository.read(cx).id()
    }

    /// Shows another repository's.
    pub fn set_repository(
        &mut self,
        repository_id: RepositoryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.repository.set(&repository_id, cx) {
            return;
        }
        self._failures = Self::follow_failures(self.repository.entity(), cx);
        let id = &repository_id;
        self.pull
            .update(cx, |button, cx| button.set_repository(id.clone(), cx));
        self.push.update(cx, |button, cx| {
            button.set_repository(id.clone(), window, cx)
        });
        self.branch.update(cx, |button, cx| {
            button.set_repository(id.clone(), window, cx)
        });
        self.stash
            .update(cx, |button, cx| button.set_repository(id.clone(), cx));
        self.update_failures(cx);
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
        let repository = self.repository.read(cx);
        self.failures
            .update(|kind| repository.change_state(kind.change()).error);
    }

    /// Whether the current branch is changing (see the type's doc).
    pub fn is_branch_busy(&self, cx: &App) -> bool {
        let repository = self.repository.read(cx);
        BranchChangeKind::ALL
            .iter()
            .any(|kind| repository.change_state(kind.change()).running)
    }

    /// The failure the current branch shows, if any.
    pub fn branch_failure(&self) -> Option<(BranchChangeKind, DataError)> {
        self.failures.shown()
    }

    /// Fetch, as it is now: spinning while it runs, red with the reason once it failed. It's the
    /// repository's: one of another stays with that repository.
    pub fn fetch_button(&self, cx: &App) -> ToolbarButton {
        let state = self.repository.read(cx).change_state(Change::Fetch);
        let pending = state.running;
        let error = state.error;
        let title = match &error {
            Some(error) => format!("Fetch failed: {error}"),
            None => "Fetch all remotes".to_owned(),
        };
        let repository = self.repository.entity().clone();
        ToolbarButton::new("fetch", IconName::RefreshCw, "Fetch")
            .hide_below(HideBelow::Small)
            .title(title)
            .disabled(pending)
            .danger(error.is_some())
            .busy(pending)
            .on_click(move |_, _, cx| {
                repository.update(cx, |repository, cx| drop(repository.fetch(cx)));
            })
    }

    fn render_current_branch(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = *cx.palette();
        let busy = self.is_branch_busy(cx);
        let repository = self.repository.read(cx);
        let name = AppData::global(cx)
            .read(cx)
            .repositories()
            .data()
            .and_then(|list| find(list, repository.id()).map(|r| r.name.clone()))
            .unwrap_or_default();
        let head = repository
            .status()
            .data()
            .map(|status| head_label(&status.head).to_owned())
            .unwrap_or_default();
        let failure = self.branch_failure();
        let (title, error) = match &failure {
            Some((kind, error)) => (kind.title(), Some(error.clone())),
            None => ("Merge", None),
        };
        let dismiss = failure.map(|(kind, _)| kind);
        let dismissed = self.repository.entity().clone();
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
        FailurePopover::new(
            "current-branch-failure",
            title,
            error.map(|error| error.to_string().into()),
            trigger,
        )
        .on_dismiss(move |_, cx| {
            if let Some(kind) = dismiss {
                dismissed.update(cx, |repository, cx| repository.dismiss(kind.change(), cx));
            }
        })
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
                    .child(
                        ToolbarButton::new("undo", IconName::Undo2, "Undo")
                            .hide_below(HideBelow::Large),
                    )
                    .child(
                        ToolbarButton::new("redo", IconName::Redo2, "Redo")
                            .hide_below(HideBelow::Large),
                    )
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

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use futures::FutureExt as _;
    use futures::channel::oneshot;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AnyWindowHandle, AppContext as _, TestAppContext};

    use super::*;
    use crate::components::repository::test_support::fake_git;
    use crate::components::repository::test_support::{
        Repositories, repository, repository_id, set_operation, set_push_target, set_refs,
        set_remotes, set_stashes, set_status, set_worktrees, setup_with, status,
    };
    use gitto_app_data::data::branch::Merge;
    use gitto_app_data::data::git::{GitTask, RepositoryGit};
    use gitto_git::features::branch::schema::MergeOutcome;
    use gitto_git::input::{BranchRef, NonEmpty};

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
        settle: RefCell<Option<oneshot::Sender<Result<(), DataError>>>>,
    }

    /// The repository `id`'s git, which fetches when the test says.
    struct FetchingGit {
        id: RepositoryId,
        fetches: Rc<Fetches>,
    }

    impl RepositoryGit for FetchingGit {
        fn fetch(&self) -> GitTask<()> {
            self.fetches.calls.borrow_mut().push(self.id.clone());
            let (settle, settled) = oneshot::channel();
            *self.fetches.settle.borrow_mut() = Some(settle);
            async move { settled.await.unwrap_or(Err(DataError::msg("dropped"))) }.boxed()
        }
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
        for id in [&a, &b] {
            let git = FetchingGit {
                id: id.clone(),
                fetches: Rc::clone(&fetches),
            };
            fake_git(id, Rc::new(git), cx);
        }
        let shown = a.clone();
        let (window, toolbar) = crate::test_support::open(cx, move |window, cx| {
            RepositoryToolbar::new(shown, window, cx)
        });
        cx.run_until_parked();
        (dir, a, b, fetches, window, toolbar)
    }

    fn fetch_button(toolbar: &Entity<RepositoryToolbar>, cx: &mut TestAppContext) -> ToolbarButton {
        toolbar.read_with(cx, |toolbar, cx| toolbar.fetch_button(cx))
    }

    fn switch(
        toolbar: &Entity<RepositoryToolbar>,
        id: &RepositoryId,
        window: AnyWindowHandle,
        cx: &mut TestAppContext,
    ) {
        cx.update_window(window, |_, window, cx| {
            toolbar.update(cx, |toolbar, cx| {
                toolbar.set_repository(id.clone(), window, cx)
            })
        })
        .unwrap();
        cx.run_until_parked();
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
        switch(&toolbar, &b, window, cx);
        assert!(!fetch_button(&toolbar, cx).is_busy());
        switch(&toolbar, &a, window, cx);
        assert!(fetch_button(&toolbar, cx).is_busy());

        let settle = fetches.settle.borrow_mut().take().unwrap();
        settle
            .send(Err(DataError::msg("Could not resolve host: example.com")))
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
    fn shows_the_last_change_of_the_branches_to_fail(cx: &mut TestAppContext) {
        /// A repository's git whose merges and deletes of branches fail.
        struct Failing;

        impl RepositoryGit for Failing {
            fn merge_branch(&self, _: BranchRef, _: NonEmpty) -> GitTask<MergeOutcome> {
                async { Err(DataError::msg("Merge conflict.")) }.boxed()
            }

            fn delete_branch(&self, _: BranchRef) -> GitTask<()> {
                async { Err(DataError::msg("Not fully merged.")) }.boxed()
            }
        }

        let (_dir, a, _, _, _, toolbar) = render(cx);
        fake_git(&a, Rc::new(Failing), cx);
        let repository = cx.update(|cx| gitto_app_data::data::Repositories::open(&a, cx));
        repository.update(cx, |repository, cx| {
            let merge = Merge {
                reference: "refs/heads/feature".into(),
                into: "main".into(),
            };
            drop(repository.merge(merge, cx));
        });
        cx.run_until_parked();
        repository.update(cx, |repository, cx| {
            drop(repository.delete_branch("refs/heads/feature".into(), cx));
        });
        cx.run_until_parked();
        let shown = toolbar.read_with(cx, |toolbar, _| toolbar.branch_failure());
        let (kind, error) = shown.unwrap();
        assert_eq!(kind.title(), "Delete branch");
        assert_eq!(error.to_string(), "Not fully merged.");

        // Dismissed, the one before it shows again.
        repository.update(cx, |repository, cx| {
            repository.dismiss(Change::DeleteBranch, cx)
        });
        let (kind, _) = toolbar
            .read_with(cx, |toolbar, _| toolbar.branch_failure())
            .unwrap();
        assert_eq!(kind.title(), "Merge");
    }

    #[test]
    fn puts_the_change_that_failed_last_first() {
        let diverged = DataError::msg("Diverged.");
        let local_changes = DataError::msg("Local changes.");
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
