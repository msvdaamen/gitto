//! The repository's history. Ported from `components/repository/history/history-table.tsx`.
//!
//! The rows are laid out once per change of what they're made of (the log, the stashes, whether
//! there are changes, HEAD, the worktrees), not per frame, and only the rows in view are rendered
//! (`VirtualList`), so a history of tens of thousands of commits scrolls as one of ten. More of it
//! loads a page at a time as its end comes near.

use std::rc::Rc;
use std::sync::Arc;

use gitto_git::features::history::schema::Commit;
use gitto_git::features::stash::schema::Stash;
use gitto_git::features::worktree::schema::Worktree;
use gitto_git::input::RepositoryId;
use gitto_ui::git::ref_labels::checkouts;
use gitto_ui::git::rows::{HistoryRow, history_graph, stash_sha, to_history_rows, with_stashes};
use gitto_ui::git::search::{matches_search, search_needle};
use gitto_ui::git::status::head_label;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    KeyDownEvent, Rems, Role, ScrollStrategy, SharedString, Subscription, Task,
    TestSupportExt as _, UniformListScrollHandle, WeakEntity, Window, div,
};

use super::columns::{CELL_PADDING, Column, HEADER_HEIGHT, LABEL_TEXT, ROW_HEIGHT, SMALL_TEXT};
use super::commit_menu::CommitMenu;
use super::commit_row::{HistoryCommitRow, RowProps};
use super::history_graph::graph_width;
use super::history_option::{MenuBuilder, RowPosition};
use super::ref_labels::{LabelActions, MoreLabels};
use super::reword_dialog::{RewordEvent, RewordMutation};
use super::stash_row::HistoryStashRow;
use super::wip_row::{HistoryWipRow, WipSummary};
use crate::components::repository::branch_menu::BranchMenuProvider;
use crate::components::ui::{
    EmptyState, IconName, LoadingState, UpdatingIndicator, VirtualList, is_near_end,
};
use crate::data::branch::{BranchChange, switch_branch_operation};
use crate::data::diff::{ChangedFiles, FilesOf, ShownFiles, changed_files_query};
use crate::data::history::{GetLog, History, git_log};
use crate::data::{Operations, Query};
use crate::state::clock::Clock;
use crate::state::delayed::{DELAY, Delayed};
use crate::theme::PaletteExt as _;
use gitto_ui::git::changes::LineCounts;

/// How many rows from the end of the history loaded so far the rows in view come before more is
/// loaded: a few screenfuls, so it's usually there before it's scrolled to.
pub const LOAD_AHEAD: usize = 100;

/// How long the count of a row's other labels is hovered before they show, so moving across the
/// history doesn't flash them; and how long they stay once the pointer leaves, so it can move
/// from the count onto them.
const MORE_OPEN_DELAY: std::time::Duration = std::time::Duration::from_millis(150);
const MORE_CLOSE_DELAY: std::time::Duration = std::time::Duration::from_millis(200);

/// What the history asks of the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryEvent {
    /// A row was selected, by its id: the uncommitted changes' (`gitto_ui::git::rows::WIP_ID`),
    /// a stash's or a commit's.
    Select(String),
}

/// What the history runs: git, or a test's.
#[derive(Clone)]
pub struct HistorySources {
    /// Reads a page of the log.
    pub log: GetLog,
    /// Switches branches, when a branch's label is double-clicked.
    pub switch: BranchChange<String>,
    /// Rewords the last commit; the app's when `None`.
    pub reword: Option<Entity<RewordMutation>>,
}

impl HistorySources {
    /// Git's.
    pub fn new(cx: &App) -> Self {
        HistorySources {
            log: git_log(cx),
            switch: switch_branch_operation(),
            reword: None,
        }
    }
}

/// The rows as laid out from what they're made of: laid out again only when that changes.
struct Layout {
    repository_id: RepositoryId,
    commits: Option<Arc<[Commit]>>,
    stashes: Arc<[Stash]>,
    worktrees: Arc<[Worktree]>,
    has_changes: bool,
    head: Option<String>,
    rows: Rc<[HistoryRow]>,
    /// The most lanes a row's graph uses, which the graph's column is sized to.
    lanes: usize,
}

/// Whether two shared lists are the same: the same list, or both empty (what's shown of one
/// that hasn't loaded).
fn same<T>(a: &Arc<[T]>, b: &Arc<[T]>) -> bool {
    Arc::ptr_eq(a, b) || (a.is_empty() && b.is_empty())
}

/// The rows a search shows, by their place among all the rows.
struct Filtered {
    needle: String,
    rows: Rc<[HistoryRow]>,
    /// `None`: all of them.
    shown: Option<Rc<[usize]>>,
}

/// Whether the rest of a row's labels are listed (see `ref_labels::MoreLabels`).
#[derive(Default)]
struct MoreLabelsState {
    /// The row whose count is hovered, or whose labels are listed.
    row: Option<String>,
    open: bool,
    hovered: bool,
    timer: Option<Task<()>>,
}

/// The repository's history: the uncommitted changes, the stashes and the commits, with their
/// graph, filtered by the toolbar's search, loaded a page at a time as it's scrolled, moved
/// through with the keyboard. An entity: it loads the log and keeps its scroll position. The page
/// tells it the search and the selection, and handles its [`HistoryEvent`]s; with nothing
/// selected, or the selected row gone (e.g. the uncommitted changes, once committed), it selects
/// the first row, so the details show it too.
pub struct HistoryTable {
    repository_id: RepositoryId,
    sources: HistorySources,
    history: History,
    search: SharedString,
    needle: String,
    selected_id: Option<String>,
    /// The row the details show: the selected one, once the selection stops moving quickly.
    details_id: Option<String>,
    /// The selected row the table last fell back from, as it was gone, and the one it fell back
    /// to.
    fell_back: Option<(Option<String>, String)>,
    layout: Layout,
    filtered: Filtered,
    /// The files of the row the details show, for its line totals.
    totals: Option<(String, Query<ChangedFiles, ShownFiles>)>,
    /// The log is reloaded whenever a branch or tag changes, which takes a while in a big
    /// repository; and more of it loads as its end comes near.
    updating: Entity<Delayed>,
    loading_more: Entity<Delayed>,
    commit_menu: Entity<CommitMenu>,
    branch_menu: Option<Entity<BranchMenuProvider>>,
    more_labels: MoreLabelsState,
    focus: FocusHandle,
    /// Whether the keyboard moved the selection last, rather than the pointer: the selected row
    /// is outlined then, while the list has the focus.
    keyboard: bool,
    scroll: UniformListScrollHandle,
    _history_subscriptions: Vec<Subscription>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<HistoryEvent> for HistoryTable {}

impl Focusable for HistoryTable {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl HistoryTable {
    pub fn new(repository_id: RepositoryId, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sources = HistorySources::new(cx);
        HistoryTable::with(repository_id, sources, window, cx)
    }

    /// Running what `sources` runs: e.g. a test's.
    pub fn with(
        repository_id: RepositoryId,
        sources: HistorySources,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let history = History::with_get(&repository_id, Rc::clone(&sources.log), cx);
        let commit_menu = cx.new(|cx| match &sources.reword {
            Some(reword) => {
                CommitMenu::with_reword(repository_id.clone(), reword.clone(), window, cx)
            }
            None => CommitMenu::new(repository_id.clone(), window, cx),
        });
        let updating = cx.new(|cx| Delayed::new(false, DELAY, cx));
        let loading_more = cx.new(|cx| Delayed::new(false, DELAY, cx));
        let branch_menu = BranchMenuProvider::current(cx);
        let clock = Clock::global(cx);
        let mut subscriptions = vec![
            cx.observe(&updating, |_, _, cx| cx.notify()),
            cx.observe(&loading_more, |_, _, cx| cx.notify()),
            cx.observe(&clock, |_, _, cx| cx.notify()),
            cx.observe(&commit_menu, |_, _, cx| cx.notify()),
            cx.subscribe_in(&commit_menu, window, |this, _, event, _, cx| match event {
                RewordEvent::Reworded { from, to } => this.replace_selected(from, to, cx),
            }),
            // A switch shows in the toolbar; a label double-clicked meanwhile does nothing.
            cx.observe_global::<Operations>(|_, cx| cx.notify()),
        ];
        if let Some(provider) = &branch_menu {
            subscriptions.push(cx.observe(provider, |this, _, cx| {
                this.recheck_more_labels(cx);
                cx.notify();
            }));
        }
        let layout = Layout {
            repository_id: repository_id.clone(),
            commits: None,
            stashes: Arc::from(Vec::new()),
            worktrees: Arc::from(Vec::new()),
            has_changes: false,
            head: None,
            rows: Rc::from(Vec::new()),
            lanes: 1,
        };
        let rows = Rc::clone(&layout.rows);
        let mut table = HistoryTable {
            repository_id,
            sources,
            history,
            search: SharedString::default(),
            needle: String::new(),
            selected_id: None,
            details_id: None,
            fell_back: None,
            layout,
            filtered: Filtered {
                needle: String::new(),
                rows,
                shown: None,
            },
            totals: None,
            updating,
            loading_more,
            commit_menu,
            branch_menu,
            more_labels: MoreLabelsState::default(),
            focus: cx.focus_handle(),
            keyboard: false,
            scroll: UniformListScrollHandle::new(),
            _history_subscriptions: Vec::new(),
            _subscriptions: subscriptions,
        };
        table.watch_history(cx);
        table.sync(cx);
        table
    }

    /// Lays the rows out again, and so on, as what they're made of changes.
    fn watch_history(&mut self, cx: &mut Context<Self>) {
        let history = &self.history;
        self._history_subscriptions = vec![
            cx.observe(history.log().query().entry(), |this, _, cx| this.sync(cx)),
            cx.observe(history.log().more(), |this, _, cx| this.sync(cx)),
            cx.observe(history.status().entry(), |this, _, cx| this.sync(cx)),
            cx.observe(history.stashes().entry(), |this, _, cx| this.sync(cx)),
            cx.observe(history.worktrees().entry(), |this, _, cx| this.sync(cx)),
        ];
    }

    pub fn repository_id(&self) -> &RepositoryId {
        &self.repository_id
    }

    /// Shows the repository `repository_id` instead, from its top.
    pub fn set_repository(
        &mut self,
        repository_id: RepositoryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.repository_id == repository_id {
            return;
        }
        self.history = History::with_get(&repository_id, Rc::clone(&self.sources.log), cx);
        self.repository_id = repository_id.clone();
        self.fell_back = None;
        self.totals = None;
        self.more_labels = MoreLabelsState::default();
        self.keyboard = false;
        self.commit_menu.update(cx, |menu, cx| {
            menu.set_repository(repository_id, window, cx)
        });
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        self.watch_history(cx);
        self.sync(cx);
        cx.notify();
    }

    pub fn set_search(&mut self, search: SharedString, cx: &mut Context<Self>) {
        if self.search != search {
            self.needle = search_needle(&search);
            self.search = search;
            self.sync(cx);
            cx.notify();
        }
    }

    pub fn set_selection(
        &mut self,
        selected_id: Option<String>,
        details_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if (&self.selected_id, &self.details_id) != (&selected_id, &details_id) {
            self.selected_id = selected_id;
            self.details_id = details_id;
            self.sync(cx);
            cx.notify();
        }
    }

    /// The history's rows, all of them.
    pub fn rows(&self) -> &Rc<[HistoryRow]> {
        &self.layout.rows
    }

    /// The rows the search shows.
    pub fn shown_rows(&self) -> Vec<&HistoryRow> {
        match &self.filtered.shown {
            None => self.layout.rows.iter().collect(),
            Some(shown) => shown.iter().map(|&ix| &self.layout.rows[ix]).collect(),
        }
    }

    fn shown_count(&self) -> usize {
        self.filtered
            .shown
            .as_ref()
            .map_or(self.layout.rows.len(), |shown| shown.len())
    }

    fn shown_row(&self, ix: usize) -> Option<&HistoryRow> {
        match &self.filtered.shown {
            None => self.layout.rows.get(ix),
            Some(shown) => shown.get(ix).map(|&ix| &self.layout.rows[ix]),
        }
    }

    /// The selected row: the one selected, or the first if it's not there.
    pub fn selected(&self) -> Option<&HistoryRow> {
        let rows = &self.layout.rows;
        self.selected_id
            .as_deref()
            .and_then(|id| rows.iter().find(|row| row.id() == id))
            .or_else(|| rows.first())
    }

    /// The selected row's place among the rows on show; `None` when the search hides it.
    fn selected_ix(&self) -> Option<usize> {
        let id = self.selected()?.id();
        match &self.filtered.shown {
            None => self.layout.rows.iter().position(|row| row.id() == id),
            Some(shown) => shown.iter().position(|&ix| self.layout.rows[ix].id() == id),
        }
    }

    /// Whose files the line totals are of, if any: the selected row's, once the details show it.
    pub fn totals_of(&self) -> Option<&str> {
        self.totals.as_ref().map(|(id, _)| id.as_str())
    }

    /// The commit menu, and its dialog.
    pub fn commit_menu(&self) -> &Entity<CommitMenu> {
        &self.commit_menu
    }

    pub fn scroll_handle(&self) -> &UniformListScrollHandle {
        &self.scroll
    }

    /// Brings what follows from the data and the selection up to date: the rows, those the search
    /// shows, falling back to the first row, the line totals' files, and what says the history
    /// is loading.
    fn sync(&mut self, cx: &mut Context<Self>) {
        self.lay_out();
        self.filter();

        // Nothing selected yet, or the selected row is gone: report the row the table falls back
        // to, so the details show it too.
        let fallback = self
            .selected()
            .filter(|row| {
                row.repository_id().as_ref() == self.repository_id.as_str()
                    && Some(row.id()) != self.selected_id.as_deref()
            })
            .map(|row| row.id().to_owned());
        if let Some(to) = fallback {
            self.fell_back = Some((self.selected_id.clone(), to.clone()));
            self.selected_id = Some(to.clone());
            cx.emit(HistoryEvent::Select(to));
        }

        self.sync_totals(cx);

        let refetching = self.history.log().query().is_refetching();
        self.updating
            .update(cx, |updating, cx| updating.set(refetching, cx));
        let more_pending = self.history.log().more().read(cx).is_pending();
        self.loading_more
            .update(cx, |loading, cx| loading.set(more_pending, cx));
        cx.notify();
    }

    /// Lays the rows out again, if what they're made of changed.
    fn lay_out(&mut self) {
        let log = self.history.commits();
        let commits = log.as_ref().map(|log| Arc::clone(&log.commits));
        let stashes = self.history.shown_stashes();
        let worktrees = self.history.shown_worktrees();
        let has_changes = self.history.has_changes();
        let head = self.history.head();
        let layout = &self.layout;
        let unchanged = layout.repository_id == self.repository_id
            && match (&layout.commits, &commits) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
            && same(&layout.stashes, &stashes)
            && same(&layout.worktrees, &worktrees)
            && layout.has_changes == has_changes
            && layout.head == head;
        if unchanged {
            return;
        }
        let rows: Vec<HistoryRow> = match &log {
            Some(log) => {
                let entries = with_stashes(&log.commits, &stashes);
                let graph = history_graph(&entries, has_changes, head.as_deref());
                let others = checkouts(&worktrees);
                let repository_id: Arc<str> = log.repository_id.as_str().into();
                to_history_rows(&repository_id, &entries, has_changes, graph, &others)
            }
            None => Vec::new(),
        };
        let lanes = rows
            .iter()
            .map(|row| row.graph().map_or(1, |graph| graph.width))
            .max()
            .unwrap_or(1)
            .max(1);
        self.layout = Layout {
            repository_id: self.repository_id.clone(),
            commits,
            stashes,
            worktrees,
            has_changes,
            head,
            rows: rows.into(),
            lanes,
        };
    }

    /// Picks the rows the search shows again, if the rows or the search changed.
    fn filter(&mut self) {
        if self.filtered.needle == self.needle && Rc::ptr_eq(&self.filtered.rows, &self.layout.rows)
        {
            return;
        }
        let needle = self.needle.clone();
        let rows = Rc::clone(&self.layout.rows);
        let shown = (!needle.is_empty()).then(|| {
            rows.iter()
                .enumerate()
                .filter(|(_, row)| matches_search(row, &needle))
                .map(|(ix, _)| ix)
                .collect::<Rc<[usize]>>()
        });
        self.filtered = Filtered {
            needle,
            rows,
            shown,
        };
    }

    /// Loads the files of the selected row once the details show it, for its line totals; for
    /// every row an arrow key held down passes, it would run git once per row.
    fn sync_totals(&mut self, cx: &mut Context<Self>) {
        let wanted = self
            .selected()
            .filter(|row| self.details_id.as_deref() == Some(row.id()))
            .and_then(|row| match row {
                HistoryRow::Commit(commit) => Some((commit.id.clone(), FilesOf::Commit)),
                HistoryRow::Stash(stash) => Some((stash.id.clone(), FilesOf::Stash)),
                HistoryRow::Wip(_) => None,
            });
        match wanted {
            None => self.totals = None,
            Some((id, of)) => {
                if self.totals_of() != Some(id.as_str()) {
                    let sha = stash_sha(&id).unwrap_or(&id).to_owned();
                    let query = changed_files_query(&self.repository_id, &sha, of, cx);
                    self.totals = Some((id, query));
                }
            }
        }
    }

    /// The selected row's line totals, once its files have loaded.
    fn totals(&self) -> Option<(String, LineCounts)> {
        let (id, query) = self.totals.as_ref()?;
        let shown = query.data()?;
        let sha = stash_sha(id).unwrap_or(id);
        (shown.shown_sha == sha).then(|| (id.clone(), shown.totals))
    }

    /// Selects the commit `to` in place of `from` if that one's selected: it was rewritten as
    /// `to`, e.g. reworded. Also once the table has fallen back from `from` to another row, which
    /// it does as soon as the history reloads without it, before it's told of `to`.
    fn replace_selected(&mut self, from: &str, to: &str, cx: &mut Context<Self>) {
        let selected = self.selected_id.as_deref();
        let fell_back_from = self.fell_back.as_ref().is_some_and(|(was, now)| {
            was.as_deref() == Some(from) && Some(now.as_str()) == selected
        });
        if selected == Some(from) || fell_back_from {
            self.select(to.to_owned(), cx);
        }
    }

    /// Selects the row `id`, telling the page.
    fn select(&mut self, id: String, cx: &mut Context<Self>) {
        self.selected_id = Some(id.clone());
        cx.emit(HistoryEvent::Select(id));
        self.sync(cx);
    }

    /// A row was clicked: it's selected, and the list takes the focus, which is what the keyboard
    /// moves through. The rows come and go as the list scrolls, and would take the focus with
    /// them.
    fn click_row(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.keyboard = false;
        self.focus.focus(window, cx);
        if self.selected().map(HistoryRow::id) != Some(id.as_str()) {
            self.select(id, cx);
        }
        cx.notify();
    }

    /// Moves the selection with the arrow keys, Page Up and Down, Home and End.
    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.modifiers.modified() {
            return;
        }
        let count = self.shown_count();
        let current = self.selected_ix().map_or(-1, |ix| ix as isize);
        let page = self.page_size(window) as isize;
        let target = match keystroke.key.as_str() {
            "down" => current + 1,
            "up" => current - 1,
            "pagedown" => current + page,
            "pageup" => current - page,
            "home" => 0,
            "end" => count as isize - 1,
            _ => return,
        };
        if count == 0 {
            return;
        }
        cx.stop_propagation();
        let ix = target.clamp(0, count as isize - 1) as usize;
        self.keyboard = true;
        if Some(ix) != self.selected_ix()
            && let Some(id) = self.shown_row(ix).map(|row| row.id().to_owned())
        {
            self.select(id, cx);
        }
        self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        cx.notify();
    }

    /// How many rows Page Up and Down move by: what fits in view, less a row to keep one in
    /// sight; at least one.
    pub fn page_size(&self, window: &Window) -> usize {
        let view = self
            .scroll
            .0
            .borrow()
            .last_item_size
            .map_or(0., |size| f32::from(size.item.height));
        let row = f32::from(ROW_HEIGHT.to_pixels(window.rem_size()));
        ((view / row).floor() as usize).saturating_sub(1).max(1)
    }

    /// Loads more of the log as its end comes near; not while searching, which only goes through
    /// what's loaded: a search that finds little would have it load page after page. A page that
    /// couldn't be loaded isn't tried again until the history is reloaded.
    fn load_more_near_end(&mut self, cx: &mut Context<Self>) {
        if self.needle.is_empty() && self.history.log().more().read(cx).error().is_none() {
            self.history.load_more(cx);
        }
    }

    /// The pointer entered or left the count of the row `row`'s other labels, or their list.
    fn hover_more_labels(&mut self, row: String, hovered: bool, cx: &mut Context<Self>) {
        let state = &mut self.more_labels;
        if state.row.as_deref() != Some(row.as_str()) {
            if !hovered {
                return;
            }
            *state = MoreLabelsState {
                row: Some(row),
                ..MoreLabelsState::default()
            };
        }
        let state = &mut self.more_labels;
        state.hovered = hovered;
        if hovered {
            if state.open {
                state.timer = None;
            } else {
                state.timer = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(MORE_OPEN_DELAY).await;
                    let _ = this.update(cx, |this, cx| {
                        if this.more_labels.hovered {
                            this.more_labels.open = true;
                            cx.notify();
                        }
                    });
                }));
            }
        } else if state.open {
            self.close_more_labels_soon(cx);
        } else {
            state.timer = None;
        }
    }

    /// Closes the list of a row's other labels after a moment, unless the pointer's back on it by
    /// then, or a branch's menu opened from it is open: the menu goes with its label.
    fn close_more_labels_soon(&mut self, cx: &mut Context<Self>) {
        self.more_labels.timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(MORE_CLOSE_DELAY).await;
            let _ = this.update(cx, |this, cx| {
                if !this.more_labels.hovered && !this.is_branch_menu_open(cx) {
                    this.more_labels.open = false;
                    cx.notify();
                }
            });
        }));
    }

    fn is_branch_menu_open(&self, cx: &App) -> bool {
        self.branch_menu
            .as_ref()
            .is_some_and(|provider| provider.read(cx).open_for().is_some())
    }

    /// A branch's menu closed: the list of other labels it was opened from closes too, once the
    /// pointer's off it.
    fn recheck_more_labels(&mut self, cx: &mut Context<Self>) {
        let state = &self.more_labels;
        if state.open && !state.hovered && state.timer.is_none() && !self.is_branch_menu_open(cx) {
            self.close_more_labels_soon(cx);
        }
    }

    /// The uncommitted changes' row's summary.
    fn wip_summary(&self) -> WipSummary {
        let status = self.history.status().data();
        WipSummary {
            counts: status.as_ref().map(|status| status.counts),
            branch: status
                .as_ref()
                .map(|status| head_label(&status.head).to_owned().into()),
        }
    }

    fn render_header(&self, graph: Rems, updating: bool, cx: &App) -> impl IntoElement {
        let palette = *cx.palette();
        let cell = |column: Column, label: &'static str| {
            column.apply(
                div()
                    .flex()
                    .items_center()
                    .h_full()
                    .px(CELL_PADDING)
                    .border_r_1()
                    .border_color(palette.border_soft)
                    .child(label),
                graph,
            )
        };
        h_flex()
            .id("history-header")
            .flex_none()
            .w_full()
            .h(HEADER_HEIGHT)
            .items_stretch()
            .border_b_1()
            .border_color(palette.border)
            .bg(palette.bg_soft)
            .text_size(LABEL_TEXT)
            .font_weight(FontWeight(720.))
            .text_color(palette.faint)
            .child(cell(Column::Labels, "BRANCH / TAG"))
            .child(cell(Column::Graph, "GRAPH"))
            .child(
                cell(Column::Message, "COMMIT MESSAGE")
                    .justify_between()
                    .gap_2()
                    .when(updating, |this| {
                        this.child(UpdatingIndicator::new("history-updating"))
                    }),
            )
            .child(cell(Column::Date, "DATE"))
    }

    /// A line below the rows, about loading more of them.
    fn render_footer(&self, cx: &App) -> Option<AnyElement> {
        let palette = *cx.palette();
        let footer = |error: bool| {
            h_flex()
                .flex_none()
                .h(ROW_HEIGHT)
                .justify_center()
                .px_3()
                .text_size(SMALL_TEXT)
                .text_color(if error { palette.coral } else { palette.faint })
        };
        if let Some(error) = self.history.log().more().read(cx).error() {
            return Some(
                footer(true)
                    .child(format!("Couldn't load more history: {}", error.message()))
                    .into_any_element(),
            );
        }
        self.loading_more.read(cx).is_on().then(|| {
            footer(false)
                .child(UpdatingIndicator::new("loading-more").label("Loading more commits…"))
                .into_any_element()
        })
    }

    fn render_rows(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let count = self.shown_count();
        let graph = graph_width(self.layout.lanes);
        let selected = self.selected().map(|row| row.id().to_owned());
        let focused = self.focus.is_focused(window);
        let table = cx.entity().downgrade();
        let switch = self.sources.switch.clone();
        let repository_id = self.repository_id.clone();
        let label_actions = LabelActions {
            switch: Rc::new({
                let repository_id = repository_id.clone();
                move |reference, _, cx| {
                    // Only a branch that isn't checked out says it can be switched to; the
                    // operation does nothing for the other.
                    let _ = switch.run(&repository_id, reference, cx);
                }
            }),
            open_worktree: Rc::new({
                let provider = self.branch_menu.clone();
                move |path, _, cx| {
                    if let Some(provider) = &provider {
                        provider.update(cx, |provider, cx| provider.open_worktree(path, cx));
                    }
                }
            }),
            menu_open_for: self
                .branch_menu
                .as_ref()
                .and_then(|provider| provider.read(cx).open_for().cloned()),
        };
        let context = Rc::new(RowsContext {
            table: table.clone(),
            rows: Rc::clone(&self.layout.rows),
            shown: self.filtered.shown.clone(),
            selected,
            outlined: self.keyboard && focused,
            graph,
            needle: self.needle.clone().into(),
            totals: self.totals(),
            wip: self.wip_summary(),
            labels: label_actions,
            commit_menu: self.commit_menu.clone(),
            commit_menu_open: self.commit_menu.read(cx).open_for().cloned(),
            more: self
                .more_labels
                .row
                .clone()
                .map(|row| (row, self.more_labels.open)),
            searching: !self.needle.is_empty(),
        });
        VirtualList::new("history-rows", count, ROW_HEIGHT, move |range, _, cx| {
            if !context.searching && is_near_end(&range, count, LOAD_AHEAD) {
                let table = context.table.clone();
                cx.defer(move |cx| {
                    let _ = table.update(cx, |table, cx| table.load_more_near_end(cx));
                });
            }
            range
                .filter_map(|ix| context.render_row(ix, count))
                .collect()
        })
        .track_scroll(&self.scroll)
        .into_any_element()
    }
}

/// What the rows in view are rendered from, shared by the list's frames.
struct RowsContext {
    table: WeakEntity<HistoryTable>,
    rows: Rc<[HistoryRow]>,
    shown: Option<Rc<[usize]>>,
    selected: Option<String>,
    outlined: bool,
    graph: Rems,
    needle: SharedString,
    totals: Option<(String, LineCounts)>,
    wip: WipSummary,
    labels: LabelActions,
    commit_menu: Entity<CommitMenu>,
    commit_menu_open: Option<SharedString>,
    more: Option<(String, bool)>,
    searching: bool,
}

impl RowsContext {
    fn render_row(&self, ix: usize, count: usize) -> Option<AnyElement> {
        let row = match &self.shown {
            None => self.rows.get(ix)?,
            Some(shown) => &self.rows[*shown.get(ix)?],
        };
        let id = row.id().to_owned();
        let selected = self.selected.as_deref() == Some(id.as_str());
        let table = self.table.clone();
        let select_id = id.clone();
        let props = RowProps {
            position: RowPosition { index: ix, count },
            selected,
            outlined: selected && self.outlined,
            graph_width: self.graph,
            needle: self.needle.clone(),
            totals: self
                .totals
                .as_ref()
                .filter(|(totals_of, _)| selected && *totals_of == id)
                .map(|(_, totals)| *totals),
            on_select: Rc::new(move |window, cx| {
                let id = select_id.clone();
                let _ = table.update(cx, |table, cx| table.click_row(id, window, cx));
            }),
        };
        Some(match row {
            HistoryRow::Wip(wip) => {
                HistoryWipRow::new(wip.clone(), props, self.wip.clone()).into_any_element()
            }
            HistoryRow::Stash(stash) => {
                HistoryStashRow::new(stash.clone(), props).into_any_element()
            }
            HistoryRow::Commit(commit) => {
                let has_more = commit.labels.len() > 1;
                let more = has_more.then(|| self.more_labels(&id));
                let menu_open = self.commit_menu_open.as_deref() == Some(id.as_str());
                let commit_menu = self.commit_menu.clone();
                let sha: SharedString = id.clone().into();
                // The menu's built when the row's right-clicked: it says then whether the commit
                // has one.
                let menu: MenuBuilder = Rc::new(move |menu, _, cx| {
                    CommitMenu::build_menu(&commit_menu, sha.clone(), menu, cx)
                });
                HistoryCommitRow::new(commit.clone(), props, self.labels.clone())
                    .more(more)
                    .menu(Some(menu), menu_open)
                    .into_any_element()
            }
        })
    }

    /// The list of the row `row`'s other labels: open if it's the one hovered, after a moment.
    fn more_labels(&self, row: &str) -> MoreLabels {
        let open = self
            .more
            .as_ref()
            .is_some_and(|(more, open)| more == row && *open);
        let table = self.table.clone();
        let row = row.to_owned();
        MoreLabels {
            open,
            on_hover: Rc::new(move |hovered, _, cx| {
                let row = row.clone();
                let _ = table.update(cx, |table, cx| table.hover_more_labels(row, hovered, cx));
            }),
        }
    }
}

impl Render for HistoryTable {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = *cx.palette();
        let log = self.history.log().query();
        let loading = log.data().is_none() && log.error().is_none();
        // Before the history's loaded, the header's sized for one lane.
        let graph = graph_width(if loading { 1 } else { self.layout.lanes });
        let updating = self.updating.read(cx).is_on();
        let body: AnyElement = if loading {
            LoadingState::new("history-loading", "Loading history…")
                .fill()
                .into_any_element()
        } else if let Some(error) = log.error().filter(|_| log.data().is_none()) {
            EmptyState::new(
                "history-error",
                IconName::TriangleAlert,
                "Couldn't load history",
            )
            .message(error.message().to_owned())
            .error()
            .into_any_element()
        } else if self.shown_count() == 0 {
            EmptyState::new("history-empty", IconName::Search, "No commits found")
                .message("Try a different message, author, or SHA.")
                .into_any_element()
        } else {
            self.render_rows(window, cx)
        };
        let footer = self.render_footer(cx);
        v_flex()
            .id("history")
            .size_full()
            .min_w_0()
            .min_h_0()
            .bg(palette.bg)
            .child(self.render_header(graph, updating, cx))
            .child(
                div()
                    .id("history-list")
                    .role(Role::ListBox)
                    .test_support()
                    .aria_label("Commit history")
                    .track_focus(&self.focus)
                    .key_context("HistoryList")
                    .on_key_down(cx.listener(Self::on_key_down))
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(body),
            )
            .children(footer)
    }
}
