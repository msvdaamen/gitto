//! A repository's page. Ported from `routes/(main)/$repoId.tsx`.

use std::rc::Rc;

use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::input::RepositoryId;
use gitto_ui_logic::git::rows::RowId;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    Context, DragMoveEvent, Entity, Focusable as _, SharedString, Subscription, Task, WeakEntity,
    Window, div, px, rems,
};

use crate::components::repository::BeforeChange;
use crate::components::repository::branch_menu::{BranchMenuEvent, BranchMenuProvider};
use crate::components::repository::details::commit_details::{CommitDetails, CommitDetailsEvent};
use crate::components::repository::diff::file_diff_view::{
    FileDiffEvent, FileDiffView, prefetch_file_diff,
};
use crate::components::repository::history::history_table::{HistoryEvent, HistoryTable};
use crate::components::repository::operation_bar::{OperationBar, OperationBarEvent};
use crate::components::repository::sidebar::refs_sidebar::RefsSidebar;
use crate::components::repository::toolbar::{RepositoryToolbar, ToolbarEvent};
use gitto_app_data::data::watch::WatchStreams;
use gitto_app_data::data::{Repositories, RepositoryWatcher};
use gitto_app_data::services::Services;
use gitto_app_data::state::panel_width::{PanelBounds, PanelWidth};
use gitto_app_data::state::router::Router;
use gitto_app_data::state::settled::{SETTLE_AFTER, Settled};
use gitto_app_kit::theme::PaletteExt as _;
use gitto_app_kit::ui::{HandleSide, PanelDrag, ResizeHandle, dragged_width};
use gitto_ui_logic::git::diff_source::DiffSource;

/// The sidebar's widths. The panels can't take more than a share of the window either (see
/// [`SIDEBAR_SHARE`]), so the history keeps room on small screens.
pub const SIDEBAR_BOUNDS: PanelBounds = PanelBounds::new(220., 160., 480.);
pub const DETAILS_BOUNDS: PanelBounds = PanelBounds::new(326., 280., 640.);
/// The most of the window the sidebar and the details take.
pub const SIDEBAR_SHARE: f32 = 0.3;
pub const DETAILS_SHARE: f32 = 0.4;
/// The window widths the layout changes at (`index.css`'s breakpoints): below `MEDIUM` the open
/// sidebar is a rail of icons; below `LARGE` the details slide over the history instead of
/// taking a column.
pub use gitto_app_kit::theme::breakpoints::{LARGE, MEDIUM};
/// The sidebar as a rail (52px), and the details over the history (340px), in rems.
pub const RAIL_WIDTH: f32 = 3.47;
pub const DETAILS_OVERLAY_WIDTH: f32 = 22.67;

/// The page's resizable panels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Sidebar,
    Details,
}

impl Panel {
    /// Its widths, as [`SIDEBAR_BOUNDS`] and [`DETAILS_BOUNDS`] say.
    pub fn bounds(self) -> PanelBounds {
        match self {
            Panel::Sidebar => SIDEBAR_BOUNDS,
            Panel::Details => DETAILS_BOUNDS,
        }
    }

    /// Which side of its edge it's on: the sidebar's left of it, the details right.
    pub fn side(self) -> HandleSide {
        match self {
            Panel::Sidebar => HandleSide::Start,
            Panel::Details => HandleSide::End,
        }
    }

    /// What its handle's called, to assistive technology.
    pub fn label(self) -> &'static str {
        match self {
            Panel::Sidebar => "Resize sidebar",
            Panel::Details => "Resize details",
        }
    }

    /// Its name: its width's saved under it (see `PanelWidth`), and its handle's id is made of
    /// it.
    pub fn name(self) -> &'static str {
        match self {
            Panel::Sidebar => "sidebar",
            Panel::Details => "details",
        }
    }
}

/// A row of a repository: tagged with its repository, as the page is kept when switching
/// repositories, and the new one starts without a selection.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RowOf {
    repository_id: RepositoryId,
    id: RowId,
}

/// The file whose changes are shown in place of the history, with the row and the list it's from.
struct OpenFile {
    repository_id: RepositoryId,
    row_id: RowId,
    source: DiffSource,
    file: ChangedFile,
    view: Entity<FileDiffView>,
    _subscription: Subscription,
}

/// A repository's page: the toolbar; the refs' sidebar, the history (or a file's changes, in its
/// place, with what's under way above both), and the selected row's details, side by side, the
/// panels resizable. Kept when switching repositories (see [`set_repository`]), so the panels and
/// the scroll positions stay as they were.
///
/// [`set_repository`]: Self::set_repository
pub struct RepositoryView {
    repository_id: RepositoryId,
    /// What tells the watcher of changes on disk: git's, or a test's.
    streams: Rc<dyn WatchStreams>,
    watcher: Entity<RepositoryWatcher>,
    search: SharedString,
    selection: Option<RowOf>,
    /// What the details show: the selected row, but only the first and the last while the
    /// selection moves quickly (an arrow key held down in the history), rather than loading every
    /// row passed. Settled with its repository, as it lags behind: the details never get another
    /// repository's row.
    settled: Entity<Settled<RowOf>>,
    /// Closed once the details show another row, or another repository, so going back to it
    /// doesn't open it again.
    open_file: Option<OpenFile>,
    /// A conflicted file to open from the operation bar, once the uncommitted changes are the row
    /// the details show: selecting them only reaches the details once the selection settles.
    resolving: Option<(RepositoryId, ChangedFile)>,
    sidebar_open: bool,
    details_open: bool,
    sidebar: PanelWidth,
    details: PanelWidth,
    /// The repository's toolbar and operation bar: made again for another repository (see
    /// [`set_repository`](Self::set_repository)).
    toolbar: Entity<RepositoryToolbar>,
    _toolbar_events: Subscription,
    operation_bar: Entity<OperationBar>,
    _operation_bar_events: Subscription,
    /// What changing the working tree waits for (see [`before_change`](Self::before_change)),
    /// and leaving the page (see `Router::set_blocker`): kept for as long as the page is.
    before_change: BeforeChange,
    /// What the branch menus in the sidebar and the history share (TS's `BranchMenuProvider`
    /// around them).
    branch_menu: Entity<BranchMenuProvider>,
    refs_sidebar: Entity<RefsSidebar>,
    history: Entity<HistoryTable>,
    commit_details: Entity<CommitDetails>,
    /// Waiting for the file on show to let go, to open another or close it.
    pending: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl RepositoryView {
    pub fn new(repository_id: RepositoryId, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let streams = Rc::new(Services::global(cx).git().clone());
        Self::with_streams(repository_id, streams, window, cx)
    }

    /// [`new`](Self::new), watching the repository through `streams`.
    pub fn with_streams(
        repository_id: RepositoryId,
        streams: Rc<dyn WatchStreams>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let storage = Rc::clone(Services::global(cx).storage());
        let id = repository_id.clone();
        let before_change = Self::before_change(cx.entity().downgrade());
        let (toolbar, toolbar_events) = Self::toolbar(&id, window, cx);
        let (operation_bar, operation_bar_events) =
            Self::operation_bar(&id, before_change.clone(), window, cx);
        let branch_menu = cx.new(|cx| BranchMenuProvider::new(id.clone(), window, cx));
        let refs_sidebar =
            cx.new(|cx| RefsSidebar::new(id.clone(), branch_menu.clone(), window, cx));
        let history = cx.new(|cx| HistoryTable::new(id.clone(), branch_menu.clone(), window, cx));
        let commit_details = cx.new(|cx| CommitDetails::new(id.clone(), window, cx));
        let settled = cx.new(|_| Settled::new(None, SETTLE_AFTER));
        let watcher = Self::watch(&repository_id, &streams, window, cx);

        let subscriptions = vec![
            cx.observe_in(&settled, window, |this, _, window, cx| {
                this.reconcile(window, cx)
            }),
            cx.subscribe_in(&history, window, |this, _, event, window, cx| match event {
                HistoryEvent::Select(id) => this.select(id.clone(), window, cx),
            }),
            cx.subscribe_in(
                &commit_details,
                window,
                |this, _, event, window, cx| match event {
                    CommitDetailsEvent::OpenFile { source, file } => {
                        if let Some(row) = this.details_id(cx) {
                            this.open_file_of(row, source.clone(), file.clone(), window, cx);
                        }
                    }
                    CommitDetailsEvent::PrefetchFile {
                        source,
                        file,
                        uncounted,
                    } => prefetch_file_diff(&this.repository_id, source, file, *uncounted, cx),
                },
            ),
            cx.subscribe_in(
                &branch_menu,
                window,
                |this, _, event, window, cx| match event {
                    BranchMenuEvent::Resolve(file) => {
                        this.resolve_conflicts(file.clone(), window, cx)
                    }
                },
            ),
            cx.observe_window_activation(window, |this, window, cx| {
                let focused = window.is_window_active();
                this.watcher
                    .update(cx, |watcher, cx| watcher.set_window_focused(focused, cx));
            }),
        ];

        branch_menu.update(cx, |menu, _| menu.set_before_change(before_change.clone()));
        // Nor is another repository opened, or the home page, before the file's edits are saved:
        // for as long as the page keeps it.
        Router::set_blocker(&before_change, cx);
        // A repository loaded before has its rows at once: the one the history falls back to is
        // read now, as it's picked before anything listens. Its events are of what it picks later.
        let selected = history.read(cx).selected().map(|row| row.id());

        let mut view = RepositoryView {
            repository_id,
            streams,
            watcher,
            search: SharedString::default(),
            selection: None,
            settled,
            open_file: None,
            resolving: None,
            sidebar_open: true,
            details_open: true,
            sidebar: PanelWidth::new(
                Panel::Sidebar.name(),
                Panel::Sidebar.bounds(),
                Rc::clone(&storage),
            ),
            details: PanelWidth::new(Panel::Details.name(), Panel::Details.bounds(), storage),
            toolbar,
            _toolbar_events: toolbar_events,
            operation_bar,
            _operation_bar_events: operation_bar_events,
            before_change,
            branch_menu,
            refs_sidebar,
            history,
            commit_details,
            pending: None,
            _subscriptions: subscriptions,
        };
        if let Some(id) = selected {
            view.select(id, window, cx);
        }
        view
    }

    /// The toolbar of the repository `id`, and the page's subscription to its events.
    fn toolbar(
        id: &RepositoryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<RepositoryToolbar>, Subscription) {
        let toolbar = cx.new(|cx| RepositoryToolbar::new(id.clone(), window, cx));
        let events = cx.subscribe_in(&toolbar, window, |this, _, event, _, cx| {
            this.on_toolbar(event, cx)
        });
        (toolbar, events)
    }

    /// The operation bar of the repository `id`, continuing and aborting once `before_change`
    /// lets them, and the page's subscription to its events.
    fn operation_bar(
        id: &RepositoryId,
        before_change: BeforeChange,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<OperationBar>, Subscription) {
        let bar = cx.new(|cx| {
            let mut bar = OperationBar::new(id.clone(), window, cx);
            bar.set_before_change(before_change);
            bar
        });
        let events = cx.subscribe_in(&bar, window, |this, _, event, window, cx| match event {
            OperationBarEvent::Resolve(file) => this.resolve_conflicts(file.clone(), window, cx),
        });
        (bar, events)
    }

    /// Starts watching the repository `id`, which comes on show now.
    fn watch(
        id: &RepositoryId,
        streams: &Rc<dyn WatchStreams>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<RepositoryWatcher> {
        let git = Rc::clone(streams);
        let focused = window.is_window_active();
        let repository = Repositories::open(id, cx);
        cx.new(|cx| RepositoryWatcher::new(&repository, git, focused, cx))
    }

    /// What closing the file, or opening another, waits for: its edits saved (see
    /// `FileDiffView::may_leave`).
    fn before_change(this: WeakEntity<Self>) -> BeforeChange {
        Rc::new(move |cx| {
            this.update(cx, |this, cx| this.may_leave_file(cx))
                .unwrap_or_else(|_| Task::ready(true))
        })
    }

    pub fn repository_id(&self) -> &RepositoryId {
        &self.repository_id
    }

    /// Shows the repository `id` instead: its own selection (none), the panels and the history's
    /// search as they are. Its toolbar and operation bar are new: nothing of the other
    /// repository's (a branch being named, the failures' order, a dialog) is left in them.
    pub fn set_repository(
        &mut self,
        id: RepositoryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.repository_id == id {
            return;
        }
        self.repository_id = id.clone();
        // A file asked for in the other one isn't opened in this one.
        self.pending = None;
        self.watcher = Self::watch(&id, &self.streams, window, cx);
        let (toolbar, toolbar_events) = Self::toolbar(&id, window, cx);
        let (sidebar, details) = (self.sidebar_open, self.details_open);
        let search = self.search.clone();
        toolbar.update(cx, |toolbar, cx| {
            toolbar.set_panels(sidebar, details, cx);
            toolbar.set_search(&search, window, cx);
        });
        self.toolbar = toolbar;
        self._toolbar_events = toolbar_events;
        let (operation_bar, operation_bar_events) =
            Self::operation_bar(&id, self.before_change.clone(), window, cx);
        self.operation_bar = operation_bar;
        self._operation_bar_events = operation_bar_events;
        self.branch_menu
            .update(cx, |menu, cx| menu.set_repository(id.clone(), window, cx));
        self.refs_sidebar.update(cx, |sidebar, cx| {
            sidebar.set_repository(id.clone(), window, cx)
        });
        self.history.update(cx, |history, cx| {
            history.set_repository(id.clone(), window, cx)
        });
        self.commit_details
            .update(cx, |details, cx| details.set_repository(id.clone(), cx));
        self.reconcile(window, cx);
    }

    /// The selected row, if it's of the repository on show.
    pub fn selected_id(&self) -> Option<RowId> {
        self.selection
            .as_ref()
            .filter(|row| row.repository_id == self.repository_id)
            .map(|row| row.id.clone())
    }

    /// The row the details show, if it's of the repository on show.
    pub fn details_id(&self, cx: &gpui_kit::App) -> Option<RowId> {
        self.settled
            .read(cx)
            .value()
            .filter(|row| row.repository_id == self.repository_id)
            .map(|row| row.id.clone())
    }

    /// The file on show, if it's of the repository and the row the details show.
    fn shown_file(&self, cx: &gpui_kit::App) -> Option<&OpenFile> {
        let details = self.details_id(cx);
        self.open_file.as_ref().filter(|open| {
            open.repository_id == self.repository_id && Some(&open.row_id) == details.as_ref()
        })
    }

    fn select(&mut self, id: RowId, window: &mut Window, cx: &mut Context<Self>) {
        let row = RowOf {
            repository_id: self.repository_id.clone(),
            id,
        };
        self.selection = Some(row.clone());
        self.settled
            .update(cx, |settled, cx| settled.set(Some(row), cx));
        self.reconcile(window, cx);
    }

    /// Brings what follows from the selection up to date: the history's and the details' rows,
    /// the file on show (closed once it's not of what the details show), and a conflicted file
    /// waiting to be opened.
    fn reconcile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.selected_id();
        let details = self.details_id(cx);
        self.history.update(cx, |history, cx| {
            history.set_selection(selected.clone(), details.clone(), cx)
        });
        self.commit_details.update(cx, |commit, cx| {
            commit.set_selected(details.clone(), window, cx)
        });

        if self.open_file.is_some() && self.shown_file(cx).is_none() {
            self.open_file = None;
        }
        // Under the file on show, the history doesn't take the keyboard, nor keep the focus.
        let shown_view = self.shown_file(cx).map(|open| open.view.clone());
        self.history.update(cx, |history, cx| {
            history.set_inert(shown_view.is_some(), cx)
        });
        if let Some(view) = shown_view
            && self.history.focus_handle(cx).contains_focused(window, cx)
        {
            view.focus_handle(cx).focus(window, cx);
        }
        let shown = self
            .shown_file(cx)
            .map(|open| (open.source.clone(), open.file.path.clone()));
        self.commit_details
            .update(cx, |commit, cx| commit.set_shown_file(shown, cx));

        if let Some((repository_id, file)) = self.resolving.clone() {
            // Not once another row, or another repository, was picked meanwhile: it was asked for
            // there.
            if repository_id != self.repository_id || selected != Some(RowId::Wip) {
                self.resolving = None;
            } else if details == Some(RowId::Wip) {
                self.resolving = None;
                self.open_file_of(RowId::Wip, DiffSource::Unstaged, file, window, cx);
            }
        }
        cx.notify();
    }

    /// Resolves to whether the file on show may be closed, or another opened.
    fn may_leave_file(&mut self, cx: &mut Context<Self>) -> Task<bool> {
        match self.shown_file(cx).map(|open| open.view.clone()) {
            Some(view) => view.update(cx, |view, cx| view.may_leave(cx)),
            None => Task::ready(true),
        }
    }

    /// Shows `file`, from `source`, the list of the row `row_id`, in place of the history, once
    /// the file on show lets go.
    fn open_file_of(
        &mut self,
        row_id: RowId,
        source: DiffSource,
        file: ChangedFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let same = self.shown_file(cx).is_some_and(|open| {
            open.row_id == row_id && open.source == source && open.file.path == file.path
        });
        if same {
            return;
        }
        let repository_id = self.repository_id.clone();
        let leave = self.may_leave_file(cx);
        self.pending = Some(cx.spawn_in(window, async move |this, cx| {
            if !leave.await {
                return;
            }
            let _ = this.update_in(cx, |this, window, cx| {
                // The row and the file are of the repository it was asked in.
                if this.repository_id != repository_id {
                    return;
                }
                // The view on show takes the other file: the last one stays on show until the
                // next one's changes are in.
                if let Some(open) = this.open_file.as_mut()
                    && open.repository_id == repository_id
                {
                    open.row_id = row_id;
                    open.source = source.clone();
                    open.file = file.clone();
                    open.view.update(cx, |view, cx| {
                        view.set_file(repository_id, source, file, cx)
                    });
                    this.reconcile(window, cx);
                    return;
                }
                let view = cx.new(|cx| {
                    FileDiffView::new(
                        repository_id.clone(),
                        source.clone(),
                        file.clone(),
                        window,
                        cx,
                    )
                });
                let subscription =
                    cx.subscribe_in(&view, window, |this, _, event, window, cx| match event {
                        FileDiffEvent::Open { source, file } => {
                            if let Some(row) =
                                this.open_file.as_ref().map(|open| open.row_id.clone())
                            {
                                this.open_file_of(row, source.clone(), file.clone(), window, cx);
                            }
                        }
                        FileDiffEvent::Close => this.close_file(window, cx),
                    });
                this.open_file = Some(OpenFile {
                    repository_id,
                    row_id,
                    source,
                    file,
                    view,
                    _subscription: subscription,
                });
                this.reconcile(window, cx);
            });
        }));
    }

    /// Goes back to the history, once the file on show lets go.
    fn close_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let leave = self.may_leave_file(cx);
        self.pending = Some(cx.spawn_in(window, async move |this, cx| {
            if leave.await {
                let _ = this.update_in(cx, |this, window, cx| {
                    this.open_file = None;
                    this.reconcile(window, cx);
                });
            }
        }));
    }

    /// Opens a conflicted file's conflicts: selects the uncommitted changes, and opens it once
    /// the details show them.
    fn resolve_conflicts(
        &mut self,
        file: ChangedFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let repository_id = self.repository_id.clone();
        let leave = self.may_leave_file(cx);
        self.pending = Some(cx.spawn_in(window, async move |this, cx| {
            if !leave.await {
                return;
            }
            let _ = this.update_in(cx, |this, window, cx| {
                if this.repository_id != repository_id {
                    return;
                }
                this.resolving = Some((repository_id, file));
                this.select(RowId::Wip, window, cx);
            });
        }));
    }

    fn on_toolbar(&mut self, event: &ToolbarEvent, cx: &mut Context<Self>) {
        match event {
            ToolbarEvent::ToggleSidebar => self.sidebar_open = !self.sidebar_open,
            ToolbarEvent::ToggleDetails => self.details_open = !self.details_open,
            ToolbarEvent::Search(search) => {
                self.search = search.clone();
                self.history
                    .update(cx, |history, cx| history.set_search(search.clone(), cx));
            }
        }
        let (sidebar, details) = (self.sidebar_open, self.details_open);
        self.toolbar
            .update(cx, |toolbar, cx| toolbar.set_panels(sidebar, details, cx));
        self.refs_sidebar
            .update(cx, |refs, cx| refs.set_open(sidebar, cx));
        cx.notify();
    }

    /// The width of `panel`.
    fn width_of(&self, panel: Panel) -> &PanelWidth {
        match panel {
            Panel::Sidebar => &self.sidebar,
            Panel::Details => &self.details,
        }
    }

    fn width_of_mut(&mut self, panel: Panel) -> &mut PanelWidth {
        match panel {
            Panel::Sidebar => &mut self.sidebar,
            Panel::Details => &mut self.details,
        }
    }

    fn on_drag_panel(&mut self, event: &DragMoveEvent<PanelDrag<Panel>>, cx: &mut Context<Self>) {
        let panel = event.drag(cx).panel;
        self.width_of_mut(panel)
            .set_width(dragged_width(event, panel.side()), false);
        cx.notify();
    }

    fn resize_handle(&self, panel: Panel, cx: &mut Context<Self>) -> ResizeHandle<Panel> {
        let width = self.width_of(panel).width();
        let this = cx.entity().downgrade();
        let reset = this.clone();
        ResizeHandle::new(
            SharedString::from(format!("resize-{}", panel.name())),
            panel,
            panel.side(),
            panel.label(),
            width,
            panel.bounds().range(),
        )
        .on_resize(move |width, save, _, cx| {
            let _ = this.update(cx, |this, cx| {
                this.width_of_mut(panel).set_width(width, save);
                cx.notify();
            });
        })
        .on_reset(move |_, cx| {
            let _ = reset.update(cx, |this, cx| {
                this.width_of_mut(panel).reset();
                cx.notify();
            });
        })
    }
}

impl Render for RepositoryView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = *cx.palette();
        // Panel widths are the window's geometry, in pixels, as they're dragged and saved.
        let viewport = f32::from(window.viewport_size().width);
        let medium = viewport >= MEDIUM;
        let large = viewport >= LARGE;
        let sidebar_width = if medium {
            px(self.sidebar.width().min(viewport * SIDEBAR_SHARE))
        } else {
            rems(RAIL_WIDTH).to_pixels(window.rem_size())
        };
        let details_width = px(self.details.width().min(viewport * DETAILS_SHARE));
        let shown_file = self.shown_file(cx).map(|open| open.view.clone());

        // The history and a file's changes share the column; the history stays underneath, so
        // it's still scrolled to where it was when the changes are closed. What's under way, a
        // merge say, is above both.
        let center = v_flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(self.operation_bar.clone())
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(self.history.clone())
                    .when_some(shown_file, |this, view| {
                        // Occluding: the history underneath takes no clicks or hovers meant for
                        // the file.
                        this.child(
                            div()
                                .id("shown-file")
                                .absolute()
                                .inset_0()
                                .bg(palette.bg)
                                .occlude()
                                .child(view),
                        )
                    }),
            );

        let sidebar_handle =
            (self.sidebar_open && medium).then(|| self.resize_handle(Panel::Sidebar, cx));
        let details_handle =
            (self.details_open && large).then(|| self.resize_handle(Panel::Details, cx));
        let details = self.details_open.then(|| {
            div()
                .h_full()
                .min_h_0()
                .overflow_hidden()
                .border_l_1()
                .border_color(palette.border)
                .bg(palette.panel)
                .child(self.commit_details.clone())
        });
        let overlay_width = rems(DETAILS_OVERLAY_WIDTH)
            .to_pixels(window.rem_size())
            .min(px(viewport) - rems(RAIL_WIDTH).to_pixels(window.rem_size()));

        v_flex()
            .size_full()
            .overflow_hidden()
            .child(self.toolbar.clone())
            .child(
                h_flex()
                    .id("repository-panels")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .items_stretch()
                    .overflow_hidden()
                    .on_drag_move(cx.listener(
                        |this, event: &DragMoveEvent<PanelDrag<Panel>>, _, cx| {
                            this.on_drag_panel(event, cx)
                        },
                    ))
                    .when(self.sidebar_open, |this| {
                        this.child(
                            div()
                                .flex_none()
                                .w(sidebar_width)
                                .h_full()
                                .border_r_1()
                                .border_color(palette.border)
                                .overflow_hidden()
                                .child(self.refs_sidebar.clone()),
                        )
                    })
                    .when_some(sidebar_handle, |this, handle| {
                        this.child(div().relative().flex_none().w_0().h_full().child(handle))
                    })
                    .child(center)
                    .when_some(details_handle, |this, handle| {
                        this.child(div().relative().flex_none().w_0().h_full().child(handle))
                    })
                    .when_some(details, |this, details| {
                        if large {
                            this.child(details.flex_none().w(details_width))
                        } else {
                            // Below `LARGE`, the details slide over the history instead, which
                            // takes none of their clicks.
                            this.child(
                                div()
                                    .id("details-overlay")
                                    .occlude()
                                    .absolute()
                                    .top_0()
                                    .right_0()
                                    .bottom_0()
                                    .w(overlay_width)
                                    .shadow_lg()
                                    .child(details.w_full()),
                            )
                        }
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    //! The page as a whole: its areas, each tested on its own, wired together. GPUI tests can't
    //! run git, so all the page shows is given to the repository's entity first (see
    //! `components::repository::test_support`).

    use std::rc::Rc;
    use std::time::Duration;

    use gitto_git::features::history::schema::Commit;
    use gitto_git::features::status::schema::Head;
    use gitto_git::schema::FileStatus;
    use gitto_ui_logic::git::diff_source::DiffFileKey;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AnyWindowHandle, Entity, TestAppContext};

    use super::*;
    use crate::components::repository::history::columns::keyed;
    use crate::components::repository::test_support::{
        Repositories, repository, repository_id, set_files, set_log, set_operation, set_patch,
        set_push_target, set_refs, set_remotes, set_stashes, set_status, set_worktrees, setup_with,
        uncommitted,
    };
    use gitto_app_data::data::Opaque;
    use gitto_app_data::data::diff::{ChangedFiles, FilesOf};
    use gitto_app_data::data::file_diff::FilePatch;
    use gitto_app_data::data::history::Log;

    fn commit(sha: &str, subject: &str, parents: &[&str], committed_at: i64) -> Commit {
        Commit {
            sha: sha.into(),
            parents: parents.iter().map(|parent| (*parent).to_owned()).collect(),
            author_name: "Ada Lovelace".into(),
            author_email: "ada@example.com".into(),
            authored_at: 0,
            committed_at,
            refs: vec![],
            subject: subject.into(),
            body: String::new(),
        }
    }

    fn added(path: &str) -> ChangedFile {
        ChangedFile {
            path: path.into(),
            status: FileStatus::Added,
            orig_path: None,
            additions: Some(1),
            deletions: Some(0),
        }
    }

    /// A repository nothing changes in on disk.
    struct Unchanged;

    impl WatchStreams for Unchanged {
        fn git_dir(
            &self,
            _: &RepositoryId,
        ) -> futures::stream::BoxStream<
            'static,
            Result<Vec<gitto_git::features::watch::schema::GitDirChange>, gitto_git::ApiError>,
        > {
            Box::pin(futures::stream::pending())
        }

        fn working_tree(
            &self,
            _: &RepositoryId,
        ) -> futures::stream::BoxStream<'static, Result<(), gitto_git::ApiError>> {
            Box::pin(futures::stream::pending())
        }
    }

    struct Page {
        _dir: tempfile::TempDir,
        window: AnyWindowHandle,
        view: Entity<RepositoryView>,
    }

    /// A repository on main at `b1` ("Second", made on "First", `a1`), each commit adding a file,
    /// without changes, loaded before: its history, and its commits' files and patches.
    fn commit_id(sha: &str) -> RowId {
        RowId::Commit(sha.to_owned())
    }

    fn loaded(cx: &mut TestAppContext) -> (tempfile::TempDir, RepositoryId) {
        let id = repository_id();
        let repositories = Rc::new(Repositories::default());
        repositories
            .list
            .borrow_mut()
            .push(repository(&id, "/work/repo"));
        let dir = setup_with(cx, repositories);
        seed(&id, cx);
        (dir, id)
    }

    /// What [`loaded`] gives the repository `id`.
    fn seed(id: &RepositoryId, cx: &mut TestAppContext) {
        let id = id.clone();
        set_status(
            &id,
            uncommitted(Head::Branch {
                name: "main".into(),
                sha: "b1".into(),
            }),
            cx,
        );
        set_stashes(&id, vec![], cx);
        set_refs(&id, vec![], cx);
        set_worktrees(&id, vec![], cx);
        set_operation(&id, None, cx);
        set_remotes(&id, &[], cx);
        set_push_target(
            &id,
            gitto_git::features::remote::schema::PushTarget::Blocked {
                reason: gitto_git::PushBlocker::NoRemote,
            },
            cx,
        );
        let log = Log {
            repository_id: id.clone(),
            commits: vec![
                commit("b1", "Second", &["a1"], 2000),
                commit("a1", "First", &[], 1000),
            ]
            .into(),
            complete: true,
        };
        set_log(&id, log, cx);
        for (sha, path) in [("b1", "second.txt"), ("a1", "first.txt")] {
            let files = ChangedFiles {
                sha: sha.into(),
                files: Opaque::new(vec![added(path)]),
            };
            set_files(&id, FilesOf::Commit, files, cx);
            let source = DiffSource::Commit { sha: sha.into() };
            let patch = FilePatch {
                key: DiffFileKey::new(&source, path),
                file: added(path),
                patch: format!(
                    "diff --git a/{path} b/{path}\nnew file mode 100644\nindex 0000000..1111111\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1 @@\n+{sha}\n"
                )
                .into(),
            };
            set_patch(&id, &source, patch, cx);
        }
    }

    /// The page of the repository [`loaded`].
    fn render(cx: &mut TestAppContext) -> Page {
        let (dir, id) = loaded(cx);
        let (window, view) = crate::test_support::open(cx, |window, cx| {
            RepositoryView::with_streams(id, Rc::new(Unchanged), window, cx)
        });
        // The history falls back to its first row as it's drawn.
        cx.update_window(window, |_, window, cx| window.render_frame(cx))
            .unwrap();
        settle(cx);
        Page {
            _dir: dir,
            window,
            view,
        }
    }

    /// Lets the selection reach the details.
    fn settle(cx: &mut TestAppContext) {
        cx.executor()
            .advance_clock(SETTLE_AFTER + Duration::from_millis(10));
        cx.run_until_parked();
    }

    impl Page {
        fn click(&self, id: impl Into<gpui_kit::ElementId>, cx: &mut TestAppContext) {
            let id = id.into();
            cx.update_window(self.window, |_, window, cx| {
                window.render_frame(cx);
                window.click(id, cx);
            })
            .unwrap();
            settle(cx);
        }

        fn selected(&self, cx: &mut TestAppContext) -> Option<RowId> {
            self.view.read_with(cx, |view, _| view.selected_id())
        }

        fn details(&self, cx: &mut TestAppContext) -> Option<RowId> {
            self.view.read_with(cx, |view, cx| view.details_id(cx))
        }

        /// The file on show in place of the history, by its source and path.
        fn shown_file(&self, cx: &mut TestAppContext) -> Option<(DiffSource, String)> {
            self.view.read_with(cx, |view, cx| {
                view.shown_file(cx)
                    .map(|open| (open.source.clone(), open.file.path.clone()))
            })
        }
    }

    #[gpui_kit::test]
    fn selects_the_row_the_history_of_a_repository_loaded_before_falls_back_to_as_its_made(
        cx: &mut TestAppContext,
    ) {
        let (_dir, id) = loaded(cx);
        crate::test_support::open(cx, |window, cx| {
            let view = RepositoryView::with_streams(id, Rc::new(Unchanged), window, cx);
            // Nothing's put off until later: the history's row is read as the page is made.
            assert_eq!(view.selected_id(), Some(commit_id("b1")));
            view
        });
    }

    #[gpui_kit::test]
    fn shows_the_details_of_the_commit_selected_in_the_history(cx: &mut TestAppContext) {
        let page = render(cx);
        // The history selects its first row, and the details show it.
        assert_eq!(page.selected(cx), Some(commit_id("b1")));
        assert_eq!(page.details(cx), Some(commit_id("b1")));

        page.click(keyed("commit", "a1"), cx);
        assert_eq!(page.selected(cx), Some(commit_id("a1")));
        assert_eq!(page.details(cx), Some(commit_id("a1")));
    }

    #[gpui_kit::test]
    fn opens_a_file_over_the_history_which_takes_none_of_its_clicks(cx: &mut TestAppContext) {
        let page = render(cx);
        page.click(keyed("commit", "a1"), cx);
        page.click("file:first.txt", cx);
        let first = DiffSource::Commit { sha: "a1".into() };
        assert_eq!(
            page.shown_file(cx),
            Some((first.clone(), "first.txt".into()))
        );

        // Where the other commit's row is, under the file: the file's, not the row's.
        page.click(keyed("commit", "b1"), cx);
        assert_eq!(page.selected(cx), Some(commit_id("a1")));
        assert_eq!(page.shown_file(cx), Some((first, "first.txt".into())));

        // Once it's closed, the history takes them again, and the details show the other commit.
        let view = page.view.clone();
        cx.update_window(page.window, |_, window, cx| {
            view.update(cx, |view, cx| view.close_file(window, cx))
        })
        .unwrap();
        settle(cx);
        assert_eq!(page.shown_file(cx), None);
        page.click(keyed("commit", "b1"), cx);
        assert_eq!(page.details(cx), Some(commit_id("b1")));
    }

    #[gpui_kit::test]
    fn leaves_the_keyboard_to_a_file_over_the_history(cx: &mut TestAppContext) {
        let page = render(cx);
        page.click(keyed("commit", "a1"), cx);
        page.click("file:first.txt", cx);
        assert!(page.shown_file(cx).is_some());

        // Even with the focus in the history, the arrows don't move its selection.
        let history = page
            .view
            .read_with(cx, |view, cx| view.history.focus_handle(cx));
        cx.update_window(page.window, |_, window, cx| {
            history.focus(window, cx);
            window.press("up", cx);
        })
        .unwrap();
        settle(cx);
        assert_eq!(page.selected(cx), Some(commit_id("a1")));
        assert!(page.shown_file(cx).is_some());
    }

    #[gpui_kit::test]
    fn doesnt_open_a_file_asked_for_in_another_repository(cx: &mut TestAppContext) {
        let page = render(cx);
        let other = repository_id();
        seed(&other, cx);
        let view = page.view.clone();
        cx.update_window(page.window, |_, window, cx| {
            view.update(cx, |view, cx| {
                let source = DiffSource::Commit { sha: "b1".into() };
                view.open_file_of(commit_id("b1"), source, added("second.txt"), window, cx);
                view.set_repository(other, window, cx);
            })
        })
        .unwrap();
        settle(cx);
        assert!(page.view.read_with(cx, |view, _| view.open_file.is_none()));
    }
}
