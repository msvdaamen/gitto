//! The repository's history. Ported from `components/repository/history/history-table.tsx`.
//!
//! The rows are laid out once per change of what they're made of (the log, the stashes, whether
//! there are changes, HEAD, the worktrees), not per frame, and only the rows in view are rendered
//! (`VirtualList`), so a history of tens of thousands of commits scrolls as one of ten. More of it
//! loads a page at a time as its end comes near.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gitto_git::features::history::schema::Commit;
use gitto_git::features::stash::schema::Stash;
use gitto_git::features::worktree::schema::Worktree;
use gitto_git::input::{BranchRef, RepositoryId};
use gitto_ui_logic::git::ref_labels::checkouts;
use gitto_ui_logic::git::rows::{HistoryRow, RowId, to_history_rows, with_stashes};
use gitto_ui_logic::git::search::{matches_search, search_needle};
use gitto_ui_logic::git::status::head_label;
use gpui_kit::base::actions::{
    SelectDown, SelectFirst, SelectLast, SelectPageDown, SelectPageUp, SelectUp,
};
use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, KeyBinding,
    Pixels, Rems, Role, ScrollHandle, ScrollStrategy, SharedString, Subscription, Task,
    TestSupportExt as _, UniformListScrollHandle, WeakEntity, Window, canvas, div, rems,
};

use super::columns::{
    CELL_PADDING, Column, HEADER_HEIGHT, LABEL_TEXT, MIN_WIDTH, ROW_HEIGHT, SMALL_TEXT,
};
use super::commit_menu::CommitMenu;
use super::commit_row::{HistoryCommitRow, RowProps};
use super::history_graph::graph_width;
use super::history_option::{MenuBuilder, RowPosition};
use super::ref_labels::{LabelActions, MoreLabels};
use super::reword_dialog::RewordEvent;
use super::stash_row::HistoryStashRow;
use super::wip_row::{HistoryWipRow, WipSummary};
use crate::components::repository::branch_menu::BranchMenuProvider;
use gitto_app_data::data::diff::{ChangedFiles, FilesOf, ShownFiles, select_shown_files};
use gitto_app_data::data::history::History;
use gitto_app_data::data::keyed::Hold;
use gitto_app_data::data::{Change, Changed, Part, Repository, Shown};
use gitto_app_data::state::clock::Clock;
use gitto_app_kit::delayed::{DELAY, Delayed};
use gitto_app_kit::theme::PaletteExt as _;
use gitto_app_kit::ui::{
    EmptyState, IconName, LoadingState, UpdatingIndicator, VirtualList, is_near_end,
};
use gitto_ui_logic::git::changes::LineCounts;

/// How many rows from the end of the history loaded so far the rows in view come before more is
/// loaded: a few screenfuls, so it's usually there before it's scrolled to.
pub const LOAD_AHEAD: usize = 100;

/// How long the count of a row's other labels is hovered before they show, so moving across the
/// history doesn't flash them; and how long they stay once the pointer leaves, so it can move
/// from the count onto them.
const MORE_OPEN_DELAY: std::time::Duration = std::time::Duration::from_millis(150);
const MORE_CLOSE_DELAY: std::time::Duration = std::time::Duration::from_millis(200);

/// The key context the history's keys are bound in (see [`bind_keys`]).
const KEY_CONTEXT: &str = "HistoryList";

/// Binds the history's keys: the arrows move the selection a row, Page Up and Down a page, Home
/// and End to the first row and the last.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", SelectUp, Some(KEY_CONTEXT)),
        KeyBinding::new("down", SelectDown, Some(KEY_CONTEXT)),
        KeyBinding::new("pageup", SelectPageUp, Some(KEY_CONTEXT)),
        KeyBinding::new("pagedown", SelectPageDown, Some(KEY_CONTEXT)),
        KeyBinding::new("home", SelectFirst, Some(KEY_CONTEXT)),
        KeyBinding::new("end", SelectLast, Some(KEY_CONTEXT)),
    ]);
}

/// What the history asks of the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryEvent {
    /// A row was selected, by its id: the uncommitted changes', a stash's or a commit's.
    Select(RowId),
}

/// What the history table shows of the repository: the history's, and whether a change (a
/// switch, say) is running.
const PARTS: &[Part] = &[
    Part::Log,
    Part::More,
    Part::Status,
    Part::Stashes,
    Part::Worktrees,
    Part::Changes,
];

/// Picks the files on show of [`ChangedFiles`], counting their lines once per list.
type SelectFiles = Box<dyn Fn(&ChangedFiles) -> ShownFiles>;

/// The rows as laid out from what they're made of: laid out again only when that changes.
struct Layout {
    repository_id: RepositoryId,
    commits: Option<Arc<[Commit]>>,
    stashes: Arc<[Stash]>,
    worktrees: Arc<[Worktree]>,
    has_changes: bool,
    head: Option<String>,
    remotes: Vec<String>,
    rows: Rc<[HistoryRow]>,
    /// Each row's place among them, by its id.
    places: HashMap<RowId, usize>,
    /// The most lanes a row's graph uses, which the graph's column is sized to.
    lanes: usize,
    /// Whether more of the log was asked for since the rows were laid out (see
    /// `RowsContext::ask_for_more`).
    asked_for_more: Rc<Cell<bool>>,
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
    /// Their places among the rows, in order; `None`: all of them.
    shown: Option<Rc<[usize]>>,
}

/// Whether the rest of a row's labels are listed (see `ref_labels::MoreLabels`).
#[derive(Default)]
struct MoreLabelsState {
    /// The commit whose count is hovered, or whose labels are listed, by its SHA.
    row: Option<String>,
    open: bool,
    /// How many of the count and the list the pointer is on. Not a flag: moving from one onto
    /// the other, the list may hear it entered before the count hears it left.
    hovered: usize,
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
    repository: Shown,
    search: SharedString,
    needle: String,
    selected_id: Option<RowId>,
    /// The row the details show: the selected one, once the selection stops moving quickly.
    details_id: Option<RowId>,
    /// The selected row the table last fell back from, as it was gone, and the one it fell back
    /// to.
    fell_back: Option<(Option<RowId>, RowId)>,
    layout: Layout,
    filtered: Filtered,
    /// The files of the row the details show, for its line totals: its id, and whose files they
    /// are, held.
    totals: Option<(RowId, FilesOf, Hold)>,
    select_files: SelectFiles,
    /// The log is reloaded whenever a branch or tag changes, which takes a while in a big
    /// repository; and more of it loads as its end comes near.
    updating: Entity<Delayed>,
    loading_more: Entity<Delayed>,
    commit_menu: Entity<CommitMenu>,
    branch_menu: Entity<BranchMenuProvider>,
    more_labels: MoreLabelsState,
    /// How many times the rows asked for more of the log (see `RowsContext::ask_for_more`).
    #[cfg(test)]
    asked_for_more: usize,
    focus: FocusHandle,
    /// Whether the keyboard moved the selection last, rather than the pointer: the selected row
    /// is outlined then, while the list has the focus.
    keyboard: bool,
    /// Under a file on show (TS's `inert`): the keyboard doesn't move its selection.
    inert: bool,
    scroll: UniformListScrollHandle,
    /// Scrolls the table sideways when its pane is narrower than the table's least width.
    sideways: ScrollHandle,
    /// The height of the list's view, as last laid out: what Page Up and Down move by.
    viewport: Rc<Cell<Pixels>>,
    _history_changes: Subscription,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<HistoryEvent> for HistoryTable {}

impl Focusable for HistoryTable {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl HistoryTable {
    /// The history of the repository `repository_id`, whose branches' labels have
    /// `branch_menu`'s menus.
    pub fn new(
        repository_id: RepositoryId,
        branch_menu: Entity<BranchMenuProvider>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let repository = Shown::new(&repository_id, PARTS, cx);
        let history_changes = HistoryTable::watch(&repository, cx);
        let commit_menu = cx.new(|cx| CommitMenu::new(repository_id.clone(), window, cx));
        let updating = cx.new(|cx| Delayed::new(false, DELAY, cx));
        let loading_more = cx.new(|cx| Delayed::new(false, DELAY, cx));
        let clock = Clock::global(cx);
        let subscriptions = vec![
            cx.observe(&updating, |_, _, cx| cx.notify()),
            cx.observe(&loading_more, |_, _, cx| cx.notify()),
            cx.observe(&clock, |_, _, cx| cx.notify()),
            cx.observe(&commit_menu, |_, _, cx| cx.notify()),
            cx.subscribe_in(&commit_menu, window, |this, _, event, _, cx| match event {
                RewordEvent::Reworded { from, to } => this.replace_selected(from, to, cx),
            }),
            cx.observe(&branch_menu, |this, _, cx| {
                this.recheck_more_labels(cx);
                cx.notify();
            }),
        ];
        let layout = Layout {
            repository_id: repository_id.clone(),
            commits: None,
            stashes: Arc::from(Vec::new()),
            worktrees: Arc::from(Vec::new()),
            has_changes: false,
            head: None,
            remotes: Vec::new(),
            rows: Rc::from(Vec::new()),
            places: HashMap::new(),
            lanes: 1,
            asked_for_more: Rc::default(),
        };
        let rows = Rc::clone(&layout.rows);
        let mut table = HistoryTable {
            repository_id,
            repository,
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
            select_files: Box::new(select_shown_files()),
            updating,
            loading_more,
            commit_menu,
            branch_menu,
            more_labels: MoreLabelsState::default(),
            #[cfg(test)]
            asked_for_more: 0,
            focus: cx.focus_handle(),
            keyboard: false,
            inert: false,
            scroll: UniformListScrollHandle::new(),
            sideways: ScrollHandle::new(),
            viewport: Rc::new(Cell::new(Pixels::ZERO)),
            _history_changes: history_changes,
            _subscriptions: subscriptions,
        };
        // The row it falls back to isn't reported: nothing listens yet. Its owner reads it
        // ([`selected`](Self::selected)) once it's made.
        table.bring_up_to_date(cx);
        table
    }

    /// Lays the rows out again, and so on, as what they're made of changes.
    fn watch(repository: &Shown, cx: &mut Context<Self>) -> Subscription {
        cx.subscribe(repository.entity(), |this, _, Changed(part), cx| {
            if History::PARTS.contains(part) {
                this.sync(cx)
            }
        })
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
        self.repository.set(&repository_id, cx);
        self._history_changes = HistoryTable::watch(&self.repository, cx);
        self.repository_id = repository_id.clone();
        self.fell_back = None;
        self.totals = None;
        self.more_labels = MoreLabelsState::default();
        self.keyboard = false;
        self.commit_menu.update(cx, |menu, cx| {
            menu.set_repository(repository_id, window, cx)
        });
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
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

    /// Leaves the keyboard alone while it's under a file on show, or takes it again.
    pub fn set_inert(&mut self, inert: bool, cx: &mut Context<Self>) {
        if self.inert != inert {
            self.inert = inert;
            cx.notify();
        }
    }

    pub fn set_selection(
        &mut self,
        selected_id: Option<RowId>,
        details_id: Option<RowId>,
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
        self.selected_row_ix().map(|ix| &self.layout.rows[ix])
    }

    /// The selected row's place among all the rows (see [`selected`](Self::selected)).
    fn selected_row_ix(&self) -> Option<usize> {
        self.selected_id
            .as_ref()
            .and_then(|id| self.layout.places.get(id).copied())
            .or_else(|| (!self.layout.rows.is_empty()).then_some(0))
    }

    /// The selected row's place among the rows on show; `None` when the search hides it.
    fn selected_ix(&self) -> Option<usize> {
        let ix = self.selected_row_ix()?;
        match &self.filtered.shown {
            None => Some(ix),
            Some(shown) => shown.binary_search(&ix).ok(),
        }
    }

    /// Whose files the line totals are of, if any: the selected row's, once the details show it.
    pub fn totals_of(&self) -> Option<&RowId> {
        self.totals.as_ref().map(|(id, _, _)| id)
    }

    /// The commit menu, and its dialog.
    pub fn commit_menu(&self) -> &Entity<CommitMenu> {
        &self.commit_menu
    }

    /// How many times the rows asked for more of the log.
    #[cfg(test)]
    pub(crate) fn asked_for_more(&self) -> usize {
        self.asked_for_more
    }

    pub fn scroll_handle(&self) -> &UniformListScrollHandle {
        &self.scroll
    }

    /// Brings what follows from the data and the selection up to date (see
    /// [`bring_up_to_date`](Self::bring_up_to_date)), and reports the row it fell back to, if it
    /// did, so the details show it too.
    fn sync(&mut self, cx: &mut Context<Self>) {
        if let Some(to) = self.bring_up_to_date(cx) {
            cx.emit(HistoryEvent::Select(to));
        }
    }

    /// Brings what follows from the data and the selection up to date: the rows, those the search
    /// shows, falling back to the first row, the line totals' files, and what says the history
    /// is loading. The row it fell back to, if it did: nothing was selected yet, or the selected
    /// row is gone.
    fn bring_up_to_date(&mut self, cx: &mut Context<Self>) -> Option<RowId> {
        self.lay_out(cx);
        self.filter();

        let fallback = self
            .selected()
            .filter(|row| {
                *row.repository_id() == self.repository_id
                    && !self.selected_id.as_ref().is_some_and(|id| row.is(id))
            })
            .map(HistoryRow::id);
        if let Some(to) = &fallback {
            self.fell_back = Some((self.selected_id.clone(), to.clone()));
            self.selected_id = Some(to.clone());
        }

        self.sync_totals(cx);

        let repository = self.repository.read(cx);
        let refetching = repository.log().is_refetching();
        let more_pending = repository.is_loading_more();
        self.updating
            .update(cx, |updating, cx| updating.set(refetching, cx));
        self.loading_more
            .update(cx, |loading, cx| loading.set(more_pending, cx));
        cx.notify();
        fallback
    }

    /// Lays the rows out again, if what they're made of changed.
    fn lay_out(&mut self, cx: &App) {
        let history = History(self.repository.read(cx));
        let log = history.commits();
        let commits = log.map(|log| Arc::clone(&log.commits));
        let stashes = history.shown_stashes();
        let worktrees = history.shown_worktrees();
        let has_changes = history.has_changes();
        let head = history.head();
        let remotes = history.shown_remotes();
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
            && layout.head == head
            && layout.remotes == remotes;
        if unchanged {
            return;
        }
        let rows: Vec<HistoryRow> = match log {
            Some(log) => {
                let entries = with_stashes(&log.commits, &stashes);
                let others = checkouts(&worktrees);
                to_history_rows(
                    &log.repository_id,
                    &entries,
                    has_changes,
                    head.as_deref(),
                    &others,
                    &remotes,
                )
            }
            None => Vec::new(),
        };
        let lanes = rows
            .iter()
            .map(|row| row.graph().map_or(1, |graph| graph.width))
            .max()
            .unwrap_or(1)
            .max(1);
        let places = rows
            .iter()
            .enumerate()
            .map(|(ix, row)| (row.id(), ix))
            .collect();
        self.layout = Layout {
            repository_id: self.repository_id.clone(),
            commits,
            stashes,
            worktrees,
            has_changes,
            head,
            remotes,
            rows: rows.into(),
            places,
            lanes,
            asked_for_more: Rc::default(),
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
            .filter(|row| self.details_id.as_ref().is_some_and(|id| row.is(id)))
            .and_then(|row| match row {
                HistoryRow::Commit(commit) => {
                    Some((RowId::Commit(commit.id.clone()), FilesOf::Commit))
                }
                HistoryRow::Stash(stash) => Some((RowId::Stash(stash.sha.clone()), FilesOf::Stash)),
                HistoryRow::Wip(_) => None,
            });
        match wanted {
            None => self.totals = None,
            Some((id, of)) => {
                if self.totals_of() != Some(&id) {
                    let sha = id.sha().unwrap_or_default().to_owned();
                    let hold = self
                        .repository
                        .update(cx, |repository, cx| repository.hold_files(of, &sha, cx));
                    self.totals = Some((id, of, hold));
                }
            }
        }
    }

    /// The selected row's line totals, once its files have loaded.
    fn totals(&self, cx: &App) -> Option<(RowId, LineCounts)> {
        let (id, of, _) = self.totals.as_ref()?;
        let files = self.repository.read(cx).files(*of, id.sha()?)?.data()?;
        let shown = (self.select_files)(files);
        (Some(shown.shown_sha.as_str()) == id.sha()).then(|| (id.clone(), shown.totals))
    }

    /// Selects the commit `to` in place of `from` if that one's selected: it was rewritten as
    /// `to`, e.g. reworded. Also once the table has fallen back from `from` to another row, which
    /// it does as soon as the history reloads without it, before it's told of `to`.
    fn replace_selected(&mut self, from: &str, to: &str, cx: &mut Context<Self>) {
        let from = RowId::Commit(from.to_owned());
        let selected = self.selected_id.as_ref();
        let fell_back_from = self
            .fell_back
            .as_ref()
            .is_some_and(|(was, now)| was.as_ref() == Some(&from) && Some(now) == selected);
        if selected == Some(&from) || fell_back_from {
            self.select(RowId::Commit(to.to_owned()), cx);
        }
    }

    /// Selects the row `id`, telling the page.
    fn select(&mut self, id: RowId, cx: &mut Context<Self>) {
        self.selected_id = Some(id.clone());
        cx.emit(HistoryEvent::Select(id));
        self.sync(cx);
    }

    /// A row was clicked: it's selected, and the list takes the focus, which is what the keyboard
    /// moves through. The rows come and go as the list scrolls, and would take the focus with
    /// them.
    fn click_row(&mut self, id: RowId, window: &mut Window, cx: &mut Context<Self>) {
        self.keyboard = false;
        self.focus.focus(window, cx);
        if !self.selected().is_some_and(|row| row.is(&id)) {
            self.select(id, cx);
        }
        cx.notify();
    }

    /// Moves the selection to the row on show `to` picks, from the selected one's place (-1 when
    /// the search hides it), among `count` rows, a page being `page` of them (see
    /// [`bind_keys`]).
    fn move_selection(
        &mut self,
        to: impl FnOnce(isize, isize, isize) -> isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let count = self.shown_count();
        if count == 0 {
            cx.propagate();
            return;
        }
        let current = self.selected_ix().map_or(-1, |ix| ix as isize);
        let page = self.page_size(window) as isize;
        let target = to(current, count as isize, page);
        let ix = target.clamp(0, count as isize - 1) as usize;
        self.keyboard = true;
        if Some(ix) != self.selected_ix()
            && let Some(id) = self.shown_row(ix).map(HistoryRow::id)
        {
            self.select(id, cx);
        }
        self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        cx.notify();
    }

    fn select_up(&mut self, _: &SelectUp, window: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(|current, _, _| current - 1, window, cx);
    }

    fn select_down(&mut self, _: &SelectDown, window: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(|current, _, _| current + 1, window, cx);
    }

    fn select_page_up(&mut self, _: &SelectPageUp, window: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(|current, _, page| current - page, window, cx);
    }

    fn select_page_down(
        &mut self,
        _: &SelectPageDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_selection(|current, _, page| current + page, window, cx);
    }

    fn select_first(&mut self, _: &SelectFirst, window: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(|_, _, _| 0, window, cx);
    }

    fn select_last(&mut self, _: &SelectLast, window: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(|_, count, _| count - 1, window, cx);
    }

    /// How many rows Page Up and Down move by: what fits in the list's view, as it was last laid
    /// out, less a row to keep one in sight; at least one.
    pub fn page_size(&self, window: &Window) -> usize {
        let view = f32::from(self.viewport.get());
        let row = f32::from(ROW_HEIGHT.to_pixels(window.rem_size()));
        ((view / row).floor() as usize).saturating_sub(1).max(1)
    }

    /// Loads more of the log as its end comes near; not while searching, which only goes through
    /// what's loaded: a search that finds little would have it load page after page (nor is it
    /// asked to, see `RowsContext::ask_for_more`). A page that couldn't be loaded isn't tried
    /// again until the history is reloaded.
    fn load_more_near_end(&mut self, cx: &mut Context<Self>) {
        #[cfg(test)]
        {
            self.asked_for_more += 1;
        }
        if self.needle.is_empty() && self.repository.read(cx).more_error().is_none() {
            self.repository
                .update(cx, |repository, cx| repository.load_more(cx));
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
        state.hovered = if hovered {
            state.hovered + 1
        } else {
            state.hovered.saturating_sub(1)
        };
        if state.hovered > 0 {
            if state.open {
                state.timer = None;
            } else {
                state.timer = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(MORE_OPEN_DELAY).await;
                    let _ = this.update(cx, |this, cx| {
                        if this.more_labels.hovered > 0 {
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
                if this.more_labels.hovered == 0 && !this.is_branch_menu_open(cx) {
                    this.more_labels.open = false;
                    cx.notify();
                }
            });
        }));
    }

    fn is_branch_menu_open(&self, cx: &App) -> bool {
        self.branch_menu.read(cx).open_for().is_some()
    }

    /// A branch's menu closed: the list of other labels it was opened from closes too, once the
    /// pointer's off it. Whatever close was timed before ran while the menu was open, and kept it.
    fn recheck_more_labels(&mut self, cx: &mut Context<Self>) {
        let state = &self.more_labels;
        if state.open && state.hovered == 0 && !self.is_branch_menu_open(cx) {
            self.close_more_labels_soon(cx);
        }
    }

    /// The uncommitted changes' row's summary.
    fn wip_summary(&self, cx: &App) -> WipSummary {
        let history = History(self.repository.read(cx));
        let status = history.summary();
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
        if let Some(error) = self.repository.read(cx).more_error() {
            return Some(
                footer(true)
                    .child(format!("Couldn't load more history: {error}"))
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
        let selected = self.selected().map(HistoryRow::id);
        let focused = self.focus.is_focused(window);
        let table = cx.entity().downgrade();
        let label_actions = LabelActions {
            switch: Rc::new({
                let repository = self.repository.entity().clone();
                move |reference, _, cx| {
                    // Only a branch that isn't checked out says it can be switched to; switching
                    // does nothing for the other. A switch shows in the toolbar; a label
                    // double-clicked meanwhile does nothing.
                    repository.update(cx, |repository, cx| {
                        let reference = BranchRef::parse(reference);
                        drop(repository.parsing(
                            Change::Switch,
                            reference,
                            Repository::switch_branch,
                            cx,
                        ))
                    });
                }
            }),
            open_worktree: Rc::new({
                let provider = self.branch_menu.clone();
                move |path, _, cx| {
                    provider.update(cx, |provider, cx| provider.open_worktree(path, cx));
                }
            }),
            branch_menu: self.branch_menu.clone(),
            menu_open_for: self.branch_menu.read(cx).open_for().cloned(),
        };
        let context = Rc::new(RowsContext {
            table: table.clone(),
            rows: Rc::clone(&self.layout.rows),
            shown: self.filtered.shown.clone(),
            selected,
            outlined: self.keyboard && focused,
            graph,
            needle: self.needle.clone().into(),
            totals: self.totals(cx),
            wip: self.wip_summary(cx),
            labels: label_actions,
            commit_menu: self.commit_menu.clone(),
            commit_menu_open: self.commit_menu.read(cx).open_for().cloned(),
            more: self
                .more_labels
                .row
                .clone()
                .map(|row| (row, self.more_labels.open)),
            searching: !self.needle.is_empty(),
            asked_for_more: Rc::clone(&self.layout.asked_for_more),
        });
        VirtualList::new("history-rows", count, ROW_HEIGHT, move |range, _, cx| {
            if is_near_end(&range, count, LOAD_AHEAD) {
                context.ask_for_more(cx);
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
    selected: Option<RowId>,
    outlined: bool,
    graph: Rems,
    needle: SharedString,
    totals: Option<(RowId, LineCounts)>,
    wip: WipSummary,
    labels: LabelActions,
    commit_menu: Entity<CommitMenu>,
    commit_menu_open: Option<SharedString>,
    more: Option<(String, bool)>,
    searching: bool,
    /// Whether more of the log was asked for since the rows were laid out.
    asked_for_more: Rc<Cell<bool>>,
}

impl RowsContext {
    /// The rows in view came near the end of the log loaded so far: asks the table for more,
    /// once per layout of the rows, rather than on every frame they're drawn, as the list's
    /// renderer is otherwise side-effect-free. Laid out again with another page, the rows ask
    /// again if they're still near the end. Not while searching.
    fn ask_for_more(&self, cx: &mut App) {
        if self.searching || self.asked_for_more.replace(true) {
            return;
        }
        let table = self.table.clone();
        cx.defer(move |cx| {
            let _ = table.update(cx, |table, cx| table.load_more_near_end(cx));
        });
    }

    fn render_row(&self, ix: usize, count: usize) -> Option<AnyElement> {
        let row = match &self.shown {
            None => self.rows.get(ix)?,
            Some(shown) => &self.rows[*shown.get(ix)?],
        };
        let id = row.id();
        let selected = self.selected.as_ref() == Some(&id);
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
                let more = has_more.then(|| self.more_labels(&commit.id));
                let menu_open = self.commit_menu_open.as_deref() == Some(commit.id.as_str());
                let commit_menu = self.commit_menu.clone();
                let sha: SharedString = commit.id.clone().into();
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
        let log = self.repository.read(cx).log();
        let loading = log.is_pending();
        let error = log
            .error()
            .filter(|_| log.data().is_none())
            .map(|error| error.to_string());
        // Before the history's loaded, the header's sized for one lane.
        let graph = graph_width(if loading { 1 } else { self.layout.lanes });
        let updating = self.updating.read(cx).is_on();
        let body: AnyElement = if loading {
            LoadingState::new("history-loading", "Loading history…")
                .fill()
                .into_any_element()
        } else if let Some(error) = error {
            EmptyState::new(
                "history-error",
                IconName::TriangleAlert,
                "Couldn't load history",
            )
            .message(error)
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
        let viewport = Rc::clone(&self.viewport);
        // The table's least width: its other columns', plus the graph's. A narrower pane scrolls
        // it sideways, header and all.
        let min_width = rems(MIN_WIDTH.0 + graph.0);
        v_flex()
            .id("history")
            .size_full()
            .min_w_0()
            .min_h_0()
            .bg(palette.bg)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(
                        div()
                            .id("history-sideways")
                            .size_full()
                            .overflow_x_scroll()
                            // Not with the wheel's vertical turns, which scroll the rows.
                            .restrict_scroll_to_axis()
                            .track_scroll(&self.sideways)
                            .child(
                                v_flex()
                                    .h_full()
                                    .w_full()
                                    .min_w(min_width)
                                    .child(self.render_header(graph, updating, cx))
                                    .child(
                                        div()
                                            .id("history-list")
                                            .role(Role::ListBox)
                                            .test_support()
                                            .aria_label("Commit history")
                                            .track_focus(&self.focus)
                                            .when(!self.inert, |list| {
                                                list.key_context(KEY_CONTEXT)
                                                    .on_action(cx.listener(Self::select_up))
                                                    .on_action(cx.listener(Self::select_down))
                                                    .on_action(cx.listener(Self::select_page_up))
                                                    .on_action(cx.listener(Self::select_page_down))
                                                    .on_action(cx.listener(Self::select_first))
                                                    .on_action(cx.listener(Self::select_last))
                                            })
                                            .relative()
                                            .flex_1()
                                            .min_h_0()
                                            .w_full()
                                            .child(body)
                                            // Measures the view, for Page Up and Down.
                                            .child(
                                                canvas(
                                                    move |bounds, _, _| {
                                                        viewport.set(bounds.size.height)
                                                    },
                                                    |_, _, _, _| {},
                                                )
                                                .absolute()
                                                .size_full(),
                                            ),
                                    ),
                            ),
                    )
                    .child(Scrollbar::horizontal(&self.sideways)),
            )
            .children(footer)
    }
}
