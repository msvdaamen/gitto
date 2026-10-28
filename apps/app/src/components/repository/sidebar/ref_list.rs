//! The sidebar's sections: branches, remotes, tags, stashes, worktrees. Ported from
//! `components/repository/sidebar/ref-list.tsx`.

use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use gitto_git::features::refs::schema::{Ref, RefKind};
use gitto_git::features::stash::schema::Stash;
use gitto_git::features::worktree::schema::Worktree;
use gitto_git::input::{BranchRef, RepositoryId};
use gitto_ui_logic::git::ref_tree::{RefTreeNode, build_ref_tree, flatten_ref_tree};
use gitto_ui_logic::git::stash::can_pop;
use gitto_ui_logic::git::status::branch_name;
use gitto_ui_logic::git::worktree::{can_open_worktree, describe_worktree, worktree_on};
use gpui_kit::base::actions::{Confirm, SelectDown, SelectFirst, SelectLast, SelectUp};
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::{WindowExt as _, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Context, DismissEvent, Entity, FocusHandle, Focusable, KeyBinding,
    ScrollStrategy, SharedString, Subscription, TestSupportExt as _, UniformListScrollHandle,
    WeakEntity, Window, actions, div,
};

use super::drop_stash_dialog::open_drop_stash_dialog;
use super::remove_worktree_dialog::open_remove_worktree_dialog;
use super::sidebar_row::{RowTag, SidebarFolder, SidebarRow};
use super::sidebar_section::{RenderRows, SidebarSection};
use super::stash_menu::StashMenu;
use super::worktree_menu::WorktreeMenu;
use crate::components::repository::branch_menu::BranchMenuProvider;
use crate::components::repository::row_menu::RowMenu;
use gitto_app_data::data::{Change, Part, Repository, Shown};
use gitto_app_data::services::Services;
use gitto_app_data::state::clock::Clock;
use gitto_app_data::state::collapsed::Collapsed;
use gitto_app_kit::theme::base_px;
use gitto_app_kit::ui::{EmptyState, IconName, LoadingState};

actions!(
    ref_list,
    [
        /// Opens the menu of the row the keyboard's on, as right-clicking it does.
        OpenRowMenu
    ]
);

/// The key context the list's keys are bound in (see [`bind_keys`]).
const KEY_CONTEXT: &str = "RefList";

/// Binds the list's keys: the arrows, Home and End move through its rows; Enter uses the row it's
/// on, as double-clicking it does (a folder, as clicking it does); Shift+F10 opens the row's
/// menu, as right-clicking it does.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", SelectUp, Some(KEY_CONTEXT)),
        KeyBinding::new("down", SelectDown, Some(KEY_CONTEXT)),
        KeyBinding::new("home", SelectFirst, Some(KEY_CONTEXT)),
        KeyBinding::new("end", SelectLast, Some(KEY_CONTEXT)),
        KeyBinding::new("enter", Confirm { secondary: false }, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-f10", OpenRowMenu, Some(KEY_CONTEXT)),
    ]);
}

/// The sidebar's sections, in the order they're shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    Local,
    Remotes,
    Tags,
    Stashes,
    Worktrees,
}

impl Section {
    pub const ALL: [Section; 5] = [
        Section::Local,
        Section::Remotes,
        Section::Tags,
        Section::Stashes,
        Section::Worktrees,
    ];

    /// The id its collapsed state is saved under, which its parts' ids are made of too.
    pub fn key(self) -> &'static str {
        match self {
            Section::Local => "local",
            Section::Remotes => "remotes",
            Section::Tags => "tags",
            Section::Stashes => "stashes",
            Section::Worktrees => "worktrees",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Section::Local => "Local branches",
            Section::Remotes => "Remotes",
            Section::Tags => "Tags",
            Section::Stashes => "Stashes",
            Section::Worktrees => "Worktrees",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Section::Local => IconName::GitBranch,
            Section::Remotes => IconName::Cloud,
            Section::Tags => IconName::Tag,
            Section::Stashes => IconName::Inbox,
            Section::Worktrees => IconName::FolderGit2,
        }
    }
}

/// A line of a ref tree, as the sidebar shows it: a folder, or a branch (labelled with its last
/// path segment).
#[derive(Debug, Clone, PartialEq)]
pub enum TreeLine {
    Folder {
        name: SharedString,
        path: SharedString,
        count: usize,
        depth: usize,
        collapsed: bool,
    },
    Branch {
        branch: Ref,
        label: SharedString,
        depth: usize,
    },
}

/// The lines of the tree of `refs`, the children of collapsed folders (by their path) left out.
pub fn tree_lines(refs: &[Ref], is_collapsed: impl Fn(&str) -> bool) -> Vec<TreeLine> {
    let tree = build_ref_tree(refs);
    flatten_ref_tree(&tree, &is_collapsed)
        .into_iter()
        .map(|row| match row.node {
            RefTreeNode::Folder(folder) => TreeLine::Folder {
                name: folder.name.clone().into(),
                path: folder.path.clone().into(),
                count: folder.count,
                depth: row.depth,
                collapsed: is_collapsed(&folder.path),
            },
            RefTreeNode::Ref(leaf) => TreeLine::Branch {
                branch: leaf.r#ref.clone(),
                label: leaf.name.clone().into(),
                depth: row.depth,
            },
        })
        .collect()
}

/// A branch's tooltip: its name, and the other worktree it's checked out in, if one is.
pub fn branch_title(branch: &Ref, worktree: Option<&Worktree>) -> String {
    match worktree {
        Some(worktree) => format!("{}\nChecked out in {}", branch.name, worktree.path),
        None => branch.name.clone(),
    }
}

/// What a branch's row says after its name: how far it's ahead of its upstream, or else behind.
pub fn branch_meta(branch: &Ref) -> Option<String> {
    if branch.ahead > 0 {
        Some(format!("↑{}", branch.ahead))
    } else if branch.behind > 0 {
        Some(format!("↓{}", branch.behind))
    } else {
        None
    }
}

/// A stash's tooltip: its message, and when it was made, relative to `now` (both in milliseconds).
pub fn stash_title(stash: &Stash, now: i64) -> String {
    format!(
        "{}\nStashed {}",
        stash.message,
        gitto_ui_logic::format::relative_time(stash.created_at, now).to_lowercase()
    )
}

/// What a worktree's row says after its name: the branch it's on, or that it's bare or detached.
pub fn worktree_meta(worktree: &Worktree) -> String {
    match worktree.branch.as_deref() {
        Some(branch) if !branch.is_empty() => branch_name(branch).to_owned(),
        _ if worktree.bare => "bare".to_owned(),
        _ => "detached".to_owned(),
    }
}

/// What the list shows of the repository.
const PARTS: &[Part] = &[
    Part::Refs,
    Part::Stashes,
    Part::Worktrees,
    Part::Status,
    Part::Changes,
];

/// A row of the list, by what it shows: what the keyboard's on, and what its element's id is
/// made of.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RowKey {
    /// A ref folder, by its path.
    Folder(SharedString),
    /// A local or remote branch, by its full ref name.
    Branch(SharedString),
    /// A tag, by its full ref name.
    Tag(SharedString),
    /// A stash, by its SHA.
    Stash(SharedString),
    /// A worktree, by its path.
    Worktree(SharedString),
}

impl RowKey {
    fn of_line(line: &TreeLine) -> RowKey {
        match line {
            TreeLine::Folder { path, .. } => RowKey::Folder(path.clone()),
            TreeLine::Branch { branch, .. } => RowKey::Branch(branch.full_name.clone().into()),
        }
    }

    /// The id of the row's element.
    fn element_id(&self) -> SharedString {
        match self {
            RowKey::Folder(path) => format!("folder-{path}"),
            RowKey::Branch(name) => format!("branch-{name}"),
            RowKey::Tag(name) => format!("tag-{name}"),
            RowKey::Stash(sha) => format!("stash-{sha}"),
            RowKey::Worktree(path) => format!("worktree-{path}"),
        }
        .into()
    }
}

/// A row's menu opened from the keyboard, until it's dismissed.
struct KeyboardMenu {
    row: RowKey,
    menu: Entity<PopupMenu>,
    _dismissed: Subscription,
}

/// The dialog open, asking about a stash or a worktree of a repository.
enum Asking {
    Drop(RepositoryId, Stash),
    Remove(RepositoryId, Worktree),
}

/// The trees' lines, made again when the refs or what's collapsed change.
#[derive(Default)]
struct Trees {
    key: Option<(Arc<[Ref]>, u64)>,
    local: Rc<[TreeLine]>,
    remote: Rc<[TreeLine]>,
    tags: Rc<[Ref]>,
    local_count: usize,
    remote_count: usize,
}

/// The worktrees other than the one on show, picked again when the worktrees change.
#[derive(Default)]
struct OtherWorktrees {
    of: Option<Arc<[Worktree]>>,
    others: Rc<[Worktree]>,
}

/// What every row of a render reads: which menu is open, the other worktrees, the row the
/// keyboard's on, and the list to act on.
struct RowContext {
    list: WeakEntity<RefList>,
    branch_menu: Entity<BranchMenuProvider>,
    /// The worktrees other than the one on show, whose files are these.
    other_worktrees: Rc<[Worktree]>,
    branch_menu_for: Option<SharedString>,
    stash_menu_for: Option<SharedString>,
    worktree_menu_for: Option<SharedString>,
    /// The row the keyboard's on, while it moves through the list.
    outlined: Option<RowKey>,
    /// A row's menu the keyboard opened, and the row.
    keyboard_menu: Option<(RowKey, Entity<PopupMenu>)>,
    now: i64,
}

impl RowContext {
    fn is_outlined(&self, row: &RowKey) -> bool {
        self.outlined.as_ref() == Some(row)
    }

    fn keyboard_menu_of(&self, row: &RowKey) -> Option<Entity<PopupMenu>> {
        self.keyboard_menu
            .as_ref()
            .filter(|(of, _)| of == row)
            .map(|(_, menu)| menu.clone())
    }

    /// What clicking the row `row` does first: the keyboard goes on from it.
    fn on_click(&self, row: RowKey) -> impl Fn(&mut Window, &mut App) + 'static {
        let list = self.list.clone();
        move |window, cx| {
            let row = row.clone();
            let _ = list.update(cx, |list, cx| list.click_row(row, window, cx));
        }
    }
}

/// The sidebar's sections: local branches, remotes, tags, stashes, worktrees (see
/// `SidebarSection`). Double-clicking a branch switches to it; for a remote one, to the local
/// branch tracking it. Right-clicking one opens a menu of what can be done with it, like creating
/// a branch from it (the `BranchMenuProvider` it's given, which the page shares with the
/// history); right-clicking a stash, one to pop or delete it. A branch checked out in another
/// worktree says so, and double-clicking it opens that worktree instead, as git won't switch to it
/// here. Double-clicking a worktree opens it, as a repository of its own; right-clicking one opens
/// a menu to open or remove it.
///
/// An entity: it shows the refs, the stashes and the worktrees of the repository it's given
/// ([`set_repository`](Self::set_repository)), keeping what's on show while they reload, so the
/// lists stay where they're scrolled to; another repository's start at the top. Sections and ref
/// folders share one saved collapsed state; folder ids are full ref paths like
/// `refs/heads/feature`, so they can't clash with the section ids. Tags and stashes start
/// collapsed.
///
/// The keyboard reaches it all: each section's header is a tab stop, and the list itself one
/// more, whose rows the arrow keys, Home and End move through, across the expanded sections; Enter
/// uses the row it's on, as double-clicking it does, and Shift+F10 opens its menu (see
/// [`bind_keys`]). Clicking a row puts the keyboard there. The rail's sections open in popovers,
/// which aren't moved through.
pub struct RefList {
    repository: Shown,
    collapsed: Collapsed,
    /// Counts the changes to `collapsed`, so the trees are made again.
    collapsed_revision: u64,
    branch_menu: Entity<BranchMenuProvider>,
    /// The stash whose menu is open, by its SHA.
    stash_menu: RowMenu,
    /// The worktree whose menu is open, by its path.
    worktree_menu: RowMenu,
    asking: Option<Asking>,
    scrolls: [UniformListScrollHandle; 5],
    trees: Trees,
    other_worktrees: OtherWorktrees,
    /// Shown as a rail of icons, which the sidebar decides (see `RefsSidebar`).
    rail: bool,
    focus: FocusHandle,
    /// The row the keyboard's on.
    cursor: Option<RowKey>,
    /// Whether the keyboard moved last, rather than the pointer: the cursor's row is outlined
    /// then, while the list has the focus.
    keyboard: bool,
    keyboard_menu: Option<KeyboardMenu>,
    _subscriptions: Vec<Subscription>,
}

impl Focusable for RefList {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl RefList {
    /// The list of the repository `repository_id`, whose branches' menus are `branch_menu`'s.
    pub fn new(
        repository_id: RepositoryId,
        branch_menu: Entity<BranchMenuProvider>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let clock = Clock::global(cx);
        let subscriptions = vec![
            cx.observe(&clock, |_, _, cx| cx.notify()),
            // Which branch's menu is open is the branch menus'.
            cx.observe(&branch_menu, |_, _, cx| cx.notify()),
        ];
        RefList {
            repository: Shown::new(&repository_id, PARTS, cx),
            collapsed: Self::collapsed_of(&repository_id, cx),
            collapsed_revision: 0,
            branch_menu,
            stash_menu: RowMenu::default(),
            worktree_menu: RowMenu::default(),
            asking: None,
            scrolls: Default::default(),
            trees: Trees::default(),
            other_worktrees: OtherWorktrees::default(),
            rail: false,
            focus: cx.focus_handle().tab_stop(true),
            cursor: None,
            keyboard: false,
            keyboard_menu: None,
            _subscriptions: subscriptions,
        }
    }

    fn collapsed_of(id: &RepositoryId, cx: &App) -> Collapsed {
        let storage = Rc::clone(Services::global(cx).storage());
        Collapsed::new(
            id,
            [(Section::Tags.key(), true), (Section::Stashes.key(), true)],
            storage,
        )
    }

    pub fn repository_id<'a>(&self, cx: &'a App) -> &'a RepositoryId {
        self.repository.id(cx)
    }

    /// Shows another repository's: its own collapsed sections, its lists at the top. A menu open
    /// for one of this one's items closes, and so does a dialog asking about one: a stash or a
    /// worktree of one repository isn't one of another.
    pub fn set_repository(
        &mut self,
        repository_id: RepositoryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.repository.set(&repository_id, cx) {
            return;
        }
        self.collapsed = Self::collapsed_of(&repository_id, cx);
        self.collapsed_revision += 1;
        self.stash_menu.close(cx);
        self.worktree_menu.close(cx);
        self.cursor = None;
        self.keyboard = false;
        if let Some(open) = self.keyboard_menu.take() {
            open.menu.update(cx, |_, cx| cx.emit(DismissEvent));
        }
        if self.asking.take().is_some() {
            window.close_dialog(cx);
        }
        for scroll in &self.scrolls {
            scroll.scroll_to_item(0, ScrollStrategy::Top);
        }
        cx.notify();
    }

    /// Shows the sections as a rail of icons, each opening its section in a popover, or not.
    pub fn set_rail(&mut self, rail: bool, cx: &mut Context<Self>) {
        if self.rail != rail {
            self.rail = rail;
            cx.notify();
        }
    }

    pub fn is_section_collapsed(&self, section: Section) -> bool {
        self.collapsed.is_collapsed(section.key())
    }

    /// Whether the ref folder at `path` (e.g. `refs/remotes/origin`) is collapsed.
    pub fn is_folder_collapsed(&self, path: &str) -> bool {
        self.collapsed.is_collapsed(path)
    }

    /// Collapses `section`, or expands it.
    pub fn toggle_section(&mut self, section: Section, cx: &mut Context<Self>) {
        self.toggle(section.key(), cx);
    }

    /// Collapses the ref folder at `path`, or expands it.
    pub fn toggle_folder(&mut self, path: &str, cx: &mut Context<Self>) {
        self.toggle(path, cx);
    }

    /// Collapses the section or folder saved as `id`, or expands it.
    fn toggle(&mut self, id: &str, cx: &mut Context<Self>) {
        self.collapsed.toggle(id);
        self.collapsed_revision += 1;
        cx.notify();
    }

    /// Whether `section` shows something running on its items: popping or deleting a stash, or
    /// adding, opening or removing a worktree.
    pub fn is_section_busy(&self, section: Section, cx: &App) -> bool {
        match section {
            Section::Stashes => StashMenu::is_busy(self.repository.read(cx)),
            Section::Worktrees => WorktreeMenu::is_busy(self.repository.read(cx)),
            Section::Local | Section::Remotes | Section::Tags => false,
        }
    }

    /// The SHA of the stash whose menu is open, if any.
    pub fn stash_menu_for(&self) -> Option<&SharedString> {
        self.stash_menu.open_for()
    }

    /// The worktrees other than the one on show.
    fn other_worktrees(&mut self, cx: &App) -> Rc<[Worktree]> {
        let worktrees = self.repository.read(cx).worktrees().data().cloned();
        let cached = &self.other_worktrees;
        let fresh = match (&cached.of, &worktrees) {
            (Some(of), Some(worktrees)) => Arc::ptr_eq(of, worktrees),
            (None, None) => true,
            _ => false,
        };
        if !fresh {
            let others = worktrees
                .as_deref()
                .unwrap_or_default()
                .iter()
                .filter(|worktree| !worktree.current)
                .cloned()
                .collect();
            self.other_worktrees = OtherWorktrees {
                of: worktrees,
                others,
            };
        }
        Rc::clone(&self.other_worktrees.others)
    }

    /// Double-clicking the branch `reference`, by its full ref name: switches to it, or opens the
    /// other worktree it's checked out in, as git won't switch to it here. The toolbar shows a
    /// switch running, and why it failed; the worktrees' section, opening the worktree.
    pub fn activate_branch(&mut self, reference: &str, cx: &mut Context<Self>) {
        let others = self.other_worktrees(cx);
        match worktree_on(&others, reference) {
            None => {
                let reference = BranchRef::parse(reference);
                self.repository.update(cx, |repository, cx| {
                    let switch = Repository::switch_branch;
                    drop(repository.parsing(Change::Switch, reference, switch, cx))
                });
            }
            Some(worktree) if can_open_worktree(worktree) => {
                let path = worktree.path.clone();
                self.branch_menu
                    .update(cx, |provider, cx| provider.open_worktree(path, cx));
            }
            Some(_) => {}
        }
    }

    /// Double-clicking the worktree at `path`: opens it, if it can be. The section shows it
    /// running, and why it failed.
    pub fn activate_worktree(&mut self, path: &str, cx: &mut Context<Self>) {
        let worktree = self
            .repository
            .read(cx)
            .worktrees()
            .data()
            .and_then(|worktrees| worktrees.iter().find(|w| w.path == path).cloned());
        if let Some(worktree) = worktree {
            WorktreeMenu::open(self.repository.entity(), &worktree, cx);
        }
    }

    /// Asks before deleting `stash`.
    fn ask_to_drop(&mut self, stash: Stash, window: &mut Window, cx: &mut Context<Self>) {
        self.asking = Some(Asking::Drop(self.repository.id(cx).clone(), stash.clone()));
        let list = cx.entity().downgrade();
        let cancelled = list.clone();
        open_drop_stash_dialog(
            &stash,
            move |_, cx| {
                let _ = list.update(cx, |list, cx| list.answer(true, cx));
            },
            move |_, cx| {
                let _ = cancelled.update(cx, |list, cx| list.answer(false, cx));
            },
            window,
            cx,
        );
    }

    /// Asks before removing `worktree`.
    fn ask_to_remove(&mut self, worktree: Worktree, window: &mut Window, cx: &mut Context<Self>) {
        self.asking = Some(Asking::Remove(
            self.repository.id(cx).clone(),
            worktree.clone(),
        ));
        let list = cx.entity().downgrade();
        let cancelled = list.clone();
        open_remove_worktree_dialog(
            &worktree,
            move |_, cx| {
                let _ = list.update(cx, |list, cx| list.answer(true, cx));
            },
            move |_, cx| {
                let _ = cancelled.update(cx, |list, cx| list.answer(false, cx));
            },
            window,
            cx,
        );
    }

    /// The dialog was answered: deletes the stash, or removes the worktree, if `confirmed`, and
    /// it's still of the repository on show. Doesn't run while another of their actions is.
    fn answer(&mut self, confirmed: bool, cx: &mut Context<Self>) {
        let Some(asking) = self.asking.take() else {
            return;
        };
        if !confirmed {
            return;
        }
        let shown = self.repository.id(cx).clone();
        let repository = self.repository.entity();
        match asking {
            Asking::Drop(id, stash) if id == shown => StashMenu::drop(repository, &stash, cx),
            Asking::Remove(id, worktree) if id == shown => {
                WorktreeMenu::remove(repository, &worktree, cx)
            }
            _ => {}
        }
    }

    /// The menu of the stash `sha`; empty, so none opens, if it's gone.
    fn stash_menu(
        list: &WeakEntity<RefList>,
        sha: SharedString,
        menu: PopupMenu,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let Some(entity) = list.upgrade() else {
            return menu;
        };
        let repository = entity.read(cx).repository.entity().clone();
        let shown = repository.read(cx);
        let stashes = shown.stashes().data();
        let Some(stash) =
            stashes.and_then(|stashes| stashes.iter().find(|stash| stash.sha == *sha).cloned())
        else {
            return menu;
        };
        // Not until both have loaded.
        let poppable = matches!(
            (shown.status().data(), stashes),
            (Some(status), Some(stashes)) if can_pop(&status.summary.counts, stashes)
        );
        RowMenu::track(
            list.clone(),
            sha,
            |list: &mut RefList| &mut list.stash_menu,
            cx,
        );
        let list = list.clone();
        StashMenu::items(
            menu,
            &repository,
            &stash.clone(),
            poppable,
            move |window, cx| {
                let stash = stash.clone();
                let _ = list.update(cx, |list, cx| list.ask_to_drop(stash, window, cx));
            },
            cx,
        )
    }

    /// The menu of the worktree at `path`; empty, so none opens, if it's gone.
    fn worktree_menu(
        list: &WeakEntity<RefList>,
        path: SharedString,
        menu: PopupMenu,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let Some(entity) = list.upgrade() else {
            return menu;
        };
        let repository = entity.read(cx).repository.entity().clone();
        let Some(worktree) = repository
            .read(cx)
            .worktrees()
            .data()
            .and_then(|worktrees| {
                worktrees
                    .iter()
                    .find(|worktree| worktree.path == *path)
                    .cloned()
            })
        else {
            return menu;
        };
        RowMenu::track(
            list.clone(),
            path,
            |list: &mut RefList| &mut list.worktree_menu,
            cx,
        );
        let list = list.clone();
        WorktreeMenu::items(
            menu,
            &repository,
            &worktree.clone(),
            move |window, cx| {
                let worktree = worktree.clone();
                let _ = list.update(cx, |list, cx| list.ask_to_remove(worktree, window, cx));
            },
            cx,
        )
    }

    /// Makes the trees' lines again if the refs or what's collapsed changed.
    fn refresh_trees(&mut self, cx: &App) {
        let Some(refs) = self.repository.read(cx).refs().data().cloned() else {
            self.trees = Trees::default();
            return;
        };
        let fresh = self.trees.key.as_ref().is_some_and(|(shown, revision)| {
            Arc::ptr_eq(shown, &refs) && *revision == self.collapsed_revision
        });
        if fresh {
            return;
        }
        let of_kind = |kind: RefKind| -> Vec<Ref> {
            refs.iter().filter(|r| r.kind == kind).cloned().collect()
        };
        let local = of_kind(RefKind::Local);
        let remote = of_kind(RefKind::Remote);
        let collapsed = &self.collapsed;
        self.trees = Trees {
            local_count: local.len(),
            remote_count: remote.len(),
            local: tree_lines(&local, |path| collapsed.is_collapsed(path)).into(),
            remote: tree_lines(&remote, |path| collapsed.is_collapsed(path)).into(),
            tags: of_kind(RefKind::Tag).into(),
            key: Some((refs, self.collapsed_revision)),
        };
    }

    /// The rows the keyboard moves through, in order, with their sections and places in them:
    /// each expanded section's, as shown. None in the rail, whose sections open in popovers.
    fn keyboard_rows(&mut self, cx: &App) -> Vec<(Section, usize, RowKey)> {
        if self.rail || self.repository.read(cx).refs().is_pending() {
            return Vec::new();
        }
        self.refresh_trees(cx);
        let repository = self.repository.read(cx);
        let mut rows = Vec::new();
        for section in Section::ALL {
            if self.is_section_collapsed(section) {
                continue;
            }
            let keys: Vec<RowKey> = match section {
                Section::Local => self.trees.local.iter().map(RowKey::of_line).collect(),
                Section::Remotes => self.trees.remote.iter().map(RowKey::of_line).collect(),
                Section::Tags => self
                    .trees
                    .tags
                    .iter()
                    .map(|tag| RowKey::Tag(tag.full_name.clone().into()))
                    .collect(),
                Section::Stashes => repository
                    .stashes()
                    .data()
                    .iter()
                    .flat_map(|stashes| stashes.iter())
                    .map(|stash| RowKey::Stash(stash.sha.clone().into()))
                    .collect(),
                Section::Worktrees => repository
                    .worktrees()
                    .data()
                    .iter()
                    .flat_map(|worktrees| worktrees.iter())
                    .map(|worktree| RowKey::Worktree(worktree.path.clone().into()))
                    .collect(),
            };
            rows.extend(
                keys.into_iter()
                    .enumerate()
                    .map(|(ix, key)| (section, ix, key)),
            );
        }
        rows
    }

    /// The row the keyboard's on, if it's still shown.
    fn cursor_row(&mut self, cx: &App) -> Option<RowKey> {
        let cursor = self.cursor.clone()?;
        self.keyboard_rows(cx)
            .into_iter()
            .any(|(_, _, key)| key == cursor)
            .then_some(cursor)
    }

    /// A row was clicked: the keyboard goes on from it, and the list takes the focus. The rows
    /// come and go as the list scrolls, and would take the focus with them. Not in the rail,
    /// whose popover would lose it.
    fn click_row(&mut self, row: RowKey, window: &mut Window, cx: &mut Context<Self>) {
        self.cursor = Some(row);
        self.keyboard = false;
        if !self.rail {
            self.focus.focus(window, cx);
        }
        cx.notify();
    }

    /// Moves the keyboard to the row `to` picks, from the place of the one it's on (if it's
    /// shown) among `count` rows, and scrolls it into view. Only while the list itself has the
    /// focus, not a section's header.
    fn move_cursor(
        &mut self,
        to: impl FnOnce(Option<usize>, usize) -> usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.focus.is_focused(window) {
            cx.propagate();
            return;
        }
        let rows = self.keyboard_rows(cx);
        if rows.is_empty() {
            return;
        }
        let current = self
            .cursor
            .as_ref()
            .and_then(|cursor| rows.iter().position(|(_, _, key)| key == cursor));
        let ix = to(current, rows.len()).min(rows.len() - 1);
        let (section, in_section, key) = &rows[ix];
        self.scrolls[*section as usize].scroll_to_item(*in_section, ScrollStrategy::Nearest);
        self.cursor = Some(key.clone());
        self.keyboard = true;
        cx.notify();
    }

    fn select_up(&mut self, _: &SelectUp, window: &mut Window, cx: &mut Context<Self>) {
        self.move_cursor(
            |current, _| current.map_or(0, |ix| ix.saturating_sub(1)),
            window,
            cx,
        );
    }

    fn select_down(&mut self, _: &SelectDown, window: &mut Window, cx: &mut Context<Self>) {
        self.move_cursor(|current, _| current.map_or(0, |ix| ix + 1), window, cx);
    }

    fn select_first(&mut self, _: &SelectFirst, window: &mut Window, cx: &mut Context<Self>) {
        self.move_cursor(|_, _| 0, window, cx);
    }

    fn select_last(&mut self, _: &SelectLast, window: &mut Window, cx: &mut Context<Self>) {
        self.move_cursor(|_, count| count - 1, window, cx);
    }

    /// Enter: uses the row the keyboard's on, as double-clicking it does; a folder's toggled, as
    /// clicking it does.
    fn confirm(&mut self, _: &Confirm, window: &mut Window, cx: &mut Context<Self>) {
        if !self.focus.is_focused(window) {
            cx.propagate();
            return;
        }
        match self.cursor_row(cx) {
            Some(RowKey::Folder(path)) => self.toggle_folder(&path, cx),
            Some(RowKey::Branch(name)) => self.activate_branch(&name, cx),
            Some(RowKey::Worktree(path)) => self.activate_worktree(&path, cx),
            Some(RowKey::Tag(_) | RowKey::Stash(_)) | None => {}
        }
    }

    /// Shift+F10: opens the menu of the row the keyboard's on, below it, as right-clicking it
    /// does; nothing for a folder or a tag, which have none.
    fn open_row_menu(&mut self, _: &OpenRowMenu, window: &mut Window, cx: &mut Context<Self>) {
        if !self.focus.is_focused(window) {
            cx.propagate();
            return;
        }
        let Some(row) = self.cursor_row(cx) else {
            return;
        };
        type Build = Box<dyn FnOnce(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>;
        let list = cx.entity().downgrade();
        let build: Build = match row.clone() {
            RowKey::Branch(name) => {
                let provider = self.branch_menu.clone();
                Box::new(move |menu, _, cx| {
                    BranchMenuProvider::build_menu(&provider, name, menu, cx)
                })
            }
            RowKey::Stash(sha) => {
                let list = list.clone();
                Box::new(move |menu, _, cx| RefList::stash_menu(&list, sha, menu, cx))
            }
            RowKey::Worktree(path) => {
                let list = list.clone();
                Box::new(move |menu, _, cx| RefList::worktree_menu(&list, path, menu, cx))
            }
            RowKey::Folder(_) | RowKey::Tag(_) => return,
        };
        // Built outside this update: the menus read the list, and track themselves on it.
        window.defer(cx, move |window, cx| {
            let menu = PopupMenu::build(window, cx, build);
            if menu.read(cx).is_empty() {
                return;
            }
            menu.focus_handle(cx).focus(window, cx);
            let _ = list.update(cx, |list, cx| {
                let dismissed =
                    cx.subscribe_in(&menu, window, |list, _, _: &DismissEvent, window, cx| {
                        list.keyboard_menu = None;
                        list.focus.focus(window, cx);
                        cx.notify();
                    });
                list.keyboard_menu = Some(KeyboardMenu {
                    row,
                    menu,
                    _dismissed: dismissed,
                });
                cx.notify();
            });
        });
    }

    fn row_context(&mut self, window: &Window, cx: &mut Context<Self>) -> Rc<RowContext> {
        let outlined = self
            .cursor
            .clone()
            .filter(|_| self.keyboard && self.focus.is_focused(window));
        Rc::new(RowContext {
            list: cx.entity().downgrade(),
            other_worktrees: self.other_worktrees(cx),
            branch_menu: self.branch_menu.clone(),
            branch_menu_for: self.branch_menu.read(cx).open_for().cloned(),
            stash_menu_for: self.stash_menu.open_for().cloned(),
            worktree_menu_for: self.worktree_menu.open_for().cloned(),
            outlined,
            keyboard_menu: self
                .keyboard_menu
                .as_ref()
                .map(|open| (open.row.clone(), open.menu.clone())),
            now: gitto_app_data::state::clock::now_ms(),
        })
    }

    /// `section` of the list, counting `count` items in `items` rows.
    fn section(
        &self,
        section: Section,
        count: usize,
        items: usize,
        render_rows: RenderRows,
        cx: &mut Context<Self>,
    ) -> SidebarSection {
        let list = cx.entity().downgrade();
        SidebarSection::new(
            section.key(),
            section.title(),
            section.icon(),
            count,
            items,
            render_rows,
        )
        .collapsed(self.is_section_collapsed(section))
        .busy(self.is_section_busy(section, cx))
        .track_scroll(&self.scrolls[section as usize])
        .on_toggle(move |_, cx| {
            let _ = list.update(cx, |list, cx| list.toggle_section(section, cx));
        })
    }
}

/// The items of a section in view: those in `range`, as far as there are any (see `VirtualList`).
fn in_view<T>(items: &[T], range: Range<usize>) -> &[T] {
    &items[range.start.min(items.len())..range.end.min(items.len())]
}

/// Renders the lines of a ref tree in `range`.
fn tree_rows(lines: Rc<[TreeLine]>, rows: Rc<RowContext>) -> RenderRows {
    Rc::new(move |range: Range<usize>, _, _| {
        in_view(&lines, range)
            .iter()
            .map(|line| tree_row(line, &rows))
            .collect()
    })
}

fn tree_row(line: &TreeLine, rows: &RowContext) -> AnyElement {
    match line {
        TreeLine::Folder {
            name,
            path,
            count,
            depth,
            collapsed,
        } => {
            let list = rows.list.clone();
            let key = RowKey::Folder(path.clone());
            let clicked = rows.on_click(key.clone());
            let path = path.clone();
            SidebarFolder::new(key.element_id(), name.clone(), *count, *depth, *collapsed)
                .outlined(rows.is_outlined(&key))
                .on_toggle(move |window, cx| {
                    clicked(window, cx);
                    let path = path.clone();
                    let _ = list.update(cx, |list, cx| list.toggle_folder(&path, cx));
                })
                .into_any_element()
        }
        TreeLine::Branch {
            branch,
            label,
            depth,
        } => branch_row(branch, label, *depth, rows),
    }
}

/// A local or remote branch's line in its tree, with its menu (see `BranchMenuProvider`). One
/// checked out in another worktree has a folder after its name, and says which in its tooltip.
fn branch_row(branch: &Ref, label: &SharedString, depth: usize, rows: &RowContext) -> AnyElement {
    let full_name = SharedString::from(branch.full_name.clone());
    let worktree = worktree_on(&rows.other_worktrees, &branch.full_name);
    let tag = worktree.map(|worktree| RowTag {
        icon: IconName::FolderGit2,
        label: format!("Checked out in {}", worktree.name).into(),
    });
    let list = rows.list.clone();
    let activated = full_name.clone();
    let menu_of = full_name.clone();
    let provider = rows.branch_menu.clone();
    let key = RowKey::Branch(full_name.clone());
    SidebarRow::new(key.element_id(), IconName::GitBranch, label.clone())
        .title(branch_title(branch, worktree))
        .depth(depth)
        .active(branch.current)
        .highlighted(rows.branch_menu_for.as_ref() == Some(&full_name))
        .tag(tag)
        .meta(branch_meta(branch).map(Into::into))
        .outlined(rows.is_outlined(&key))
        .on_click(rows.on_click(key.clone()))
        .on_double_click(move |_, cx| {
            let _ = list.update(cx, |list, cx| list.activate_branch(&activated, cx));
        })
        .menu(move |menu, _, cx| {
            BranchMenuProvider::build_menu(&provider, menu_of.clone(), menu, cx)
        })
        .keyboard_menu(rows.keyboard_menu_of(&key))
        .into_any_element()
}

impl Render for RefList {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rail = self.rail;
        self.refresh_trees(cx);
        let rows = self.row_context(window, cx);
        let repository = self.repository.entity().clone();
        let refs = repository.read(cx).refs();

        let error = refs.error().map(|error| {
            div().h(base_px(150.)).flex_none().child(
                EmptyState::new(
                    "refs-error",
                    IconName::TriangleAlert,
                    "Couldn't load branches",
                )
                .error()
                .message(error.to_string())
                .fill(),
            )
        });
        let list = v_flex()
            .id("ref-list")
            .test_support()
            .track_focus(&self.focus)
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::select_first))
            .on_action(cx.listener(Self::select_last))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::open_row_menu))
            .size_full()
            .min_h_0()
            .when(rail, |list| list.items_center().gap_1().pt_2())
            .children(error);
        if refs.is_pending() {
            return list.child(
                div()
                    .h(base_px(150.))
                    .flex_none()
                    .child(LoadingState::new("refs-loading", "Loading branches…").fill()),
            );
        }

        let local = self
            .section(
                Section::Local,
                self.trees.local_count,
                self.trees.local.len(),
                tree_rows(Rc::clone(&self.trees.local), Rc::clone(&rows)),
                cx,
            )
            .rail(rail);
        let remotes = self
            .section(
                Section::Remotes,
                self.trees.remote_count,
                self.trees.remote.len(),
                tree_rows(Rc::clone(&self.trees.remote), Rc::clone(&rows)),
                cx,
            )
            .rail(rail);
        let tags = Rc::clone(&self.trees.tags);
        let tag_rows = {
            let tags = Rc::clone(&tags);
            let rows = Rc::clone(&rows);
            Rc::new(move |range: Range<usize>, _: &mut Window, _: &mut App| {
                in_view(&tags, range)
                    .iter()
                    .map(|tag| {
                        let key = RowKey::Tag(tag.full_name.clone().into());
                        SidebarRow::new(key.element_id(), IconName::Tag, tag.name.clone())
                            .outlined(rows.is_outlined(&key))
                            .on_click(rows.on_click(key))
                            .into_any_element()
                    })
                    .collect()
            })
        };
        let tags = self
            .section(Section::Tags, tags.len(), tags.len(), tag_rows, cx)
            .rail(rail);

        let stashes: Arc<[Stash]> = repository
            .read(cx)
            .stashes()
            .data()
            .cloned()
            .unwrap_or_else(|| Arc::new([]));
        let stash_rows = {
            let stashes = Arc::clone(&stashes);
            let rows = Rc::clone(&rows);
            Rc::new(move |range: Range<usize>, _: &mut Window, _: &mut App| {
                in_view(&stashes, range)
                    .iter()
                    .map(|stash| stash_row(stash, &rows))
                    .collect()
            })
        };
        let stashes = self
            .section(
                Section::Stashes,
                stashes.len(),
                stashes.len(),
                stash_rows,
                cx,
            )
            .rail(rail)
            .failure(StashMenu::failure(&repository, cx));

        let worktrees: Arc<[Worktree]> = repository
            .read(cx)
            .worktrees()
            .data()
            .cloned()
            .unwrap_or_else(|| Arc::new([]));
        let worktree_rows = {
            let worktrees = Arc::clone(&worktrees);
            let rows = Rc::clone(&rows);
            Rc::new(move |range: Range<usize>, _: &mut Window, _: &mut App| {
                in_view(&worktrees, range)
                    .iter()
                    .map(|worktree| worktree_row(worktree, &rows))
                    .collect()
            })
        };
        let worktrees = self
            .section(
                Section::Worktrees,
                worktrees.len(),
                worktrees.len(),
                worktree_rows,
                cx,
            )
            .rail(rail)
            .failure(WorktreeMenu::failure(&repository, cx));

        list.child(local)
            .child(remotes)
            .child(tags)
            .child(stashes)
            .child(worktrees)
    }
}

/// A stash's line, with its menu.
fn stash_row(stash: &Stash, rows: &RowContext) -> AnyElement {
    let sha = SharedString::from(stash.sha.clone());
    let list = rows.list.clone();
    let menu_of = sha.clone();
    let key = RowKey::Stash(sha.clone());
    SidebarRow::new(key.element_id(), IconName::Archive, stash.message.clone())
        .title(stash_title(stash, rows.now))
        .highlighted(rows.stash_menu_for.as_ref() == Some(&sha))
        .outlined(rows.is_outlined(&key))
        .on_click(rows.on_click(key.clone()))
        .menu(move |menu, _, cx| RefList::stash_menu(&list, menu_of.clone(), menu, cx))
        .keyboard_menu(rows.keyboard_menu_of(&key))
        .into_any_element()
}

/// A worktree's line, with its menu: the one on show active, one whose folder is gone faded.
fn worktree_row(worktree: &Worktree, rows: &RowContext) -> AnyElement {
    let path = SharedString::from(worktree.path.clone());
    let list = rows.list.clone();
    let opened = list.clone();
    let menu_of = path.clone();
    let activated = path.clone();
    let key = RowKey::Worktree(path.clone());
    SidebarRow::new(
        key.element_id(),
        IconName::FolderGit2,
        worktree.name.clone(),
    )
    .title(describe_worktree(worktree))
    .active(worktree.current)
    .highlighted(rows.worktree_menu_for.as_ref() == Some(&path))
    .faded(worktree.prunable.is_some())
    .meta(Some(worktree_meta(worktree).into()))
    .outlined(rows.is_outlined(&key))
    .on_click(rows.on_click(key.clone()))
    // The section shows it running, and why it failed.
    .on_double_click(move |_, cx| {
        let _ = opened.update(cx, |list, cx| list.activate_worktree(&activated, cx));
    })
    .menu(move |menu, _, cx| RefList::worktree_menu(&list, menu_of.clone(), menu, cx))
    .keyboard_menu(rows.keyboard_menu_of(&key))
    .into_any_element()
}

#[cfg(test)]
mod tests;
