//! A repository's page. Ported from `routes/(main)/$repoId.tsx`.

use std::rc::Rc;

use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::input::RepositoryId;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    Context, DragMoveEvent, Entity, SharedString, Subscription, Task, WeakEntity, Window, div, px,
    rems,
};

use crate::components::repository::BeforeChange;
use crate::components::repository::details::commit_details::{CommitDetails, CommitDetailsEvent};
use crate::components::repository::diff::file_diff_view::{FileDiffEvent, FileDiffView};
use crate::components::repository::history::history_table::{HistoryEvent, HistoryTable};
use crate::components::repository::operation_bar::{OperationBar, OperationBarEvent};
use crate::components::repository::sidebar::refs_sidebar::RefsSidebar;
use crate::components::repository::toolbar::{RepositoryToolbar, ToolbarEvent};
use crate::components::ui::{HandleSide, PanelDrag, ResizeHandle, dragged_width};
use crate::data::{QueryClient, RepositoryWatcher};
use crate::git::diff_source::DiffSource;
use crate::git::rows::WIP_ID;
use crate::services::Services;
use crate::state::panel_width::{PanelBounds, PanelWidth};
use crate::state::router::Router;
use crate::state::settled::{SETTLE_AFTER, Settled};
use crate::theme::PaletteExt as _;

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
pub const MEDIUM: f32 = 900.;
pub const LARGE: f32 = 1100.;
/// The sidebar as a rail (52px), and the details over the history (340px), in rems.
pub const RAIL_WIDTH: f32 = 3.47;
pub const DETAILS_OVERLAY_WIDTH: f32 = 22.67;

/// A row of a repository: tagged with its repository, as the page is kept when switching
/// repositories, and the new one starts without a selection.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RowOf {
    repository_id: RepositoryId,
    id: String,
}

/// The file whose changes are shown in place of the history, with the row and the list it's from.
struct OpenFile {
    repository_id: RepositoryId,
    row_id: String,
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
    toolbar: Entity<RepositoryToolbar>,
    operation_bar: Entity<OperationBar>,
    refs_sidebar: Entity<RefsSidebar>,
    history: Entity<HistoryTable>,
    commit_details: Entity<CommitDetails>,
    /// Waiting for the file on show to let go, to open another or close it.
    pending: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl RepositoryView {
    pub fn new(repository_id: RepositoryId, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let storage = Rc::clone(Services::global(cx).storage());
        let id = repository_id.clone();
        let toolbar = cx.new(|cx| RepositoryToolbar::new(id.clone(), window, cx));
        let operation_bar = cx.new(|cx| OperationBar::new(id.clone(), window, cx));
        let refs_sidebar = cx.new(|cx| RefsSidebar::new(id.clone(), window, cx));
        let history = cx.new(|cx| HistoryTable::new(id.clone(), window, cx));
        let commit_details = cx.new(|cx| CommitDetails::new(id.clone(), window, cx));
        let settled = cx.new(|_| Settled::new(None, SETTLE_AFTER));
        let watcher = Self::watch(&repository_id, window, cx);

        let subscriptions = vec![
            cx.observe_in(&settled, window, |this, _, window, cx| {
                this.reconcile(window, cx)
            }),
            cx.subscribe_in(&toolbar, window, |this, _, event, _, cx| {
                this.on_toolbar(event, cx)
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
                    // Prefetching a file's changes is the file diff view's, once it's built.
                    CommitDetailsEvent::PrefetchFile { .. } => {}
                },
            ),
            cx.subscribe_in(
                &operation_bar,
                window,
                |this, _, event, window, cx| match event {
                    OperationBarEvent::Resolve(file) => {
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

        let before_change = Self::before_change(cx.entity().downgrade());
        operation_bar.update(cx, |bar, _| bar.set_before_change(before_change.clone()));
        // Nor is another repository opened, or the home page, before the file's edits are saved.
        Router::set_blocker(Some(before_change), cx);

        RepositoryView {
            repository_id,
            watcher,
            search: SharedString::default(),
            selection: None,
            settled,
            open_file: None,
            resolving: None,
            sidebar_open: true,
            details_open: true,
            sidebar: PanelWidth::new("sidebar", SIDEBAR_BOUNDS, Rc::clone(&storage)),
            details: PanelWidth::new("details", DETAILS_BOUNDS, storage),
            toolbar,
            operation_bar,
            refs_sidebar,
            history,
            commit_details,
            pending: None,
            _subscriptions: subscriptions,
        }
    }

    /// Starts watching the repository `id`, which comes on show now.
    fn watch(
        id: &RepositoryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<RepositoryWatcher> {
        let git: Rc<dyn crate::data::watch::WatchStreams> =
            Rc::new(Services::global(cx).git().clone());
        let focused = window.is_window_active();
        let shown_at = QueryClient::now(cx);
        let id = id.clone();
        cx.new(|cx| RepositoryWatcher::new(id, git, focused, shown_at, cx))
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

    /// Shows the repository `id` instead: its own selection (none), the panels as they are.
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
        self.watcher = Self::watch(&id, window, cx);
        self.toolbar
            .update(cx, |toolbar, cx| toolbar.set_repository(id.clone(), cx));
        self.operation_bar
            .update(cx, |bar, cx| bar.set_repository(id.clone(), cx));
        self.refs_sidebar
            .update(cx, |sidebar, cx| sidebar.set_repository(id.clone(), cx));
        self.history
            .update(cx, |history, cx| history.set_repository(id.clone(), cx));
        self.commit_details
            .update(cx, |details, cx| details.set_repository(id.clone(), cx));
        self.reconcile(window, cx);
    }

    /// The selected row, if it's of the repository on show.
    pub fn selected_id(&self) -> Option<String> {
        self.selection
            .as_ref()
            .filter(|row| row.repository_id == self.repository_id)
            .map(|row| row.id.clone())
    }

    /// The row the details show, if it's of the repository on show.
    pub fn details_id(&self, cx: &gpui_kit::App) -> Option<String> {
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

    fn select(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
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
        let shown = self
            .shown_file(cx)
            .map(|open| (open.source.clone(), open.file.path.clone()));
        self.commit_details
            .update(cx, |commit, cx| commit.set_shown_file(shown, cx));

        if let Some((repository_id, file)) = self.resolving.clone() {
            // Not once another row, or another repository, was picked meanwhile: it was asked for
            // there.
            if repository_id != self.repository_id || selected.as_deref() != Some(WIP_ID) {
                self.resolving = None;
            } else if details.as_deref() == Some(WIP_ID) {
                self.resolving = None;
                self.open_file_of(WIP_ID.to_owned(), DiffSource::Unstaged, file, window, cx);
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
        row_id: String,
        source: DiffSource,
        file: ChangedFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let same = self.shown_file(cx).is_some_and(|open| {
            open.row_id == row_id && open.source.is_same(&source) && open.file.path == file.path
        });
        if same {
            return;
        }
        let leave = self.may_leave_file(cx);
        self.pending = Some(cx.spawn_in(window, async move |this, cx| {
            if !leave.await {
                return;
            }
            let _ = this.update_in(cx, |this, window, cx| {
                let repository_id = this.repository_id.clone();
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
        let leave = self.may_leave_file(cx);
        self.pending = Some(cx.spawn_in(window, async move |this, cx| {
            if !leave.await {
                return;
            }
            let _ = this.update_in(cx, |this, window, cx| {
                this.resolving = Some((this.repository_id.clone(), file));
                this.select(WIP_ID.to_owned(), window, cx);
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

    fn on_drag_panel(&mut self, event: &DragMoveEvent<PanelDrag>, cx: &mut Context<Self>) {
        if let Some(width) = dragged_width(event, "sidebar", HandleSide::Start, cx) {
            self.sidebar.set_width(width, false);
        } else if let Some(width) = dragged_width(event, "details", HandleSide::End, cx) {
            self.details.set_width(width, false);
        }
        cx.notify();
    }

    fn resize_handle(&self, panel: &'static str, cx: &mut Context<Self>) -> ResizeHandle {
        let (side, label, width) = match panel {
            "sidebar" => (HandleSide::Start, "Resize sidebar", self.sidebar.width()),
            _ => (HandleSide::End, "Resize details", self.details.width()),
        };
        let bounds = if panel == "sidebar" {
            SIDEBAR_BOUNDS
        } else {
            DETAILS_BOUNDS
        };
        let this = cx.entity().downgrade();
        let reset = this.clone();
        ResizeHandle::new(panel, side, label, width, bounds)
            .on_resize(move |width, save, _, cx| {
                let _ = this.update(cx, |this, cx| {
                    match panel {
                        "sidebar" => this.sidebar.set_width(width, save),
                        _ => this.details.set_width(width, save),
                    }
                    cx.notify();
                });
            })
            .on_reset(move |_, cx| {
                let _ = reset.update(cx, |this, cx| {
                    match panel {
                        "sidebar" => this.sidebar.reset(),
                        _ => this.details.reset(),
                    }
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
                        this.child(div().absolute().inset_0().bg(palette.bg).child(view))
                    }),
            );

        let sidebar_handle =
            (self.sidebar_open && medium).then(|| self.resize_handle("sidebar", cx));
        let details_handle =
            (self.details_open && large).then(|| self.resize_handle("details", cx));
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
                    .on_drag_move(
                        cx.listener(|this, event: &DragMoveEvent<PanelDrag>, _, cx| {
                            this.on_drag_panel(event, cx)
                        }),
                    )
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
                            // Below `LARGE`, the details slide over the history instead.
                            this.child(
                                div()
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
