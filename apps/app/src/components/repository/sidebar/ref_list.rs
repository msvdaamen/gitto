//! The sidebar's sections: branches, remotes, tags, stashes, worktrees. Ported from
//! `components/repository/sidebar/ref-list.tsx`.

use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use gitto_git::features::refs::schema::{Ref, RefKind};
use gitto_git::features::stash::schema::Stash;
use gitto_git::features::worktree::schema::Worktree;
use gitto_git::input::RepositoryId;
use gitto_ui_logic::git::ref_tree::{RefTreeNode, build_ref_tree, flatten_ref_tree};
use gitto_ui_logic::git::stash::can_pop;
use gitto_ui_logic::git::status::branch_name;
use gitto_ui_logic::git::worktree::{can_open_worktree, describe_worktree, worktree_on};
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Context, ScrollStrategy, SharedString, Subscription, UniformListScrollHandle,
    WeakEntity, Window, div,
};

use super::base_px;
use super::drop_stash_dialog::open_drop_stash_dialog;
use super::remove_worktree_dialog::open_remove_worktree_dialog;
use super::sidebar_row::{RowTag, SidebarFolder, SidebarRow};
use super::sidebar_section::{RenderRows, SidebarSection};
use super::stash_menu::StashMenu;
use super::worktree_menu::WorktreeMenu;
use crate::components::repository::branch_menu::{BranchMenuProvider, branch_menu};
use crate::components::repository::row_menu::RowMenu;
use crate::views::repository::MEDIUM;
use gitto_app_data::data::{Part, Shown};
use gitto_app_data::services::Services;
use gitto_app_data::state::clock::Clock;
use gitto_app_data::state::collapsed::Collapsed;
use gitto_app_kit::ui::{EmptyState, IconName, LoadingState};

/// The sections, by the id their collapsed state is saved under.
const LOCAL: &str = "local";
const REMOTES: &str = "remotes";
const TAGS: &str = "tags";
const STASHES: &str = "stashes";
const WORKTREES: &str = "worktrees";
const SECTIONS: [&str; 5] = [LOCAL, REMOTES, TAGS, STASHES, WORKTREES];

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
    Part::Operations,
];

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

/// What every row of a render reads: which menu is open, the other worktrees, and the list to
/// act on.
struct RowContext {
    list: WeakEntity<RefList>,
    /// The worktrees other than the one on show, whose files are these.
    other_worktrees: Vec<Worktree>,
    branch_menu_for: Option<SharedString>,
    stash_menu_for: Option<SharedString>,
    worktree_menu_for: Option<SharedString>,
    now: i64,
}

/// The sidebar's sections: local branches, remotes, tags, stashes, worktrees (see
/// `SidebarSection`). Double-clicking a branch switches to it; for a remote one, to the local
/// branch tracking it. Right-clicking one opens a menu of what can be done with it, like creating
/// a branch from it (`branch_menu`); right-clicking a stash, one to pop or delete it. A branch
/// checked out in another worktree says so, and double-clicking it opens that worktree instead, as
/// git won't switch to it here. Double-clicking a worktree opens it, as a repository of its own;
/// right-clicking one opens a menu to open or remove it.
///
/// An entity: it shows the refs, the stashes and the worktrees of the repository it's given
/// ([`set_repository`](Self::set_repository)), keeping what's on show while they reload, so the
/// lists stay where they're scrolled to; another repository's start at the top. Sections and ref
/// folders share one saved collapsed state; folder ids are full ref paths like
/// `refs/heads/feature`, so they can't clash with the section ids. Tags and stashes start
/// collapsed.
pub struct RefList {
    repository: Shown,
    collapsed: Collapsed,
    /// Counts the changes to `collapsed`, so the trees are made again.
    collapsed_revision: u64,
    /// The stash whose menu is open, by its SHA.
    stash_menu: RowMenu,
    /// The worktree whose menu is open, by its path.
    worktree_menu: RowMenu,
    asking: Option<Asking>,
    scrolls: [UniformListScrollHandle; 5],
    trees: Trees,
    _subscriptions: Vec<Subscription>,
}

impl RefList {
    pub fn new(repository_id: RepositoryId, _: &mut Window, cx: &mut Context<Self>) -> Self {
        let clock = Clock::global(cx);
        let mut subscriptions = vec![cx.observe(&clock, |_, _, cx| cx.notify())];
        // Which branch's menu is open is the branch menus' (see `branch_menu`).
        if let Some(provider) = BranchMenuProvider::current(cx) {
            subscriptions.push(cx.observe(&provider, |_, _, cx| cx.notify()));
        }
        RefList {
            repository: Shown::new(&repository_id, PARTS, cx),
            collapsed: Self::collapsed_of(&repository_id, cx),
            collapsed_revision: 0,
            stash_menu: RowMenu::default(),
            worktree_menu: RowMenu::default(),
            asking: None,
            scrolls: Default::default(),
            trees: Trees::default(),
            _subscriptions: subscriptions,
        }
    }

    fn collapsed_of(id: &RepositoryId, cx: &App) -> Collapsed {
        let storage = Rc::clone(Services::global(cx).storage());
        Collapsed::new(id.to_string(), [(TAGS, true), (STASHES, true)], storage)
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
        if self.asking.take().is_some() {
            use gpui_kit::component::WindowExt as _;
            window.close_dialog(cx);
        }
        for scroll in &self.scrolls {
            scroll.scroll_to_item(0, ScrollStrategy::Top);
        }
        cx.notify();
    }

    /// Whether the section or folder `id` is collapsed.
    pub fn is_collapsed(&self, id: &str) -> bool {
        self.collapsed.is_collapsed(id)
    }

    /// Collapses the section or folder `id`, or expands it.
    pub fn toggle(&mut self, id: &str, cx: &mut Context<Self>) {
        self.collapsed.toggle(id);
        self.collapsed_revision += 1;
        cx.notify();
    }

    /// Whether the section `id` shows something running on its items: popping or deleting a
    /// stash, or adding, opening or removing a worktree.
    pub fn is_section_busy(&self, id: &str, cx: &App) -> bool {
        match id {
            STASHES => StashMenu::is_busy(self.repository.read(cx)),
            WORKTREES => WorktreeMenu::is_busy(self.repository.read(cx)),
            _ => false,
        }
    }

    /// The SHA of the stash whose menu is open, if any.
    pub fn stash_menu_for(&self) -> Option<&SharedString> {
        self.stash_menu.open_for()
    }

    /// The worktrees other than the one on show.
    fn other_worktrees(&self, cx: &App) -> Vec<Worktree> {
        self.repository
            .read(cx)
            .worktrees()
            .data()
            .map(|worktrees| {
                worktrees
                    .iter()
                    .filter(|worktree| !worktree.current)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Double-clicking the branch `reference`, by its full ref name: switches to it, or opens the
    /// other worktree it's checked out in, as git won't switch to it here. The toolbar shows a
    /// switch running, and why it failed; the worktrees' section, opening the worktree.
    pub fn activate_branch(&mut self, reference: &str, cx: &mut Context<Self>) {
        let others = self.other_worktrees(cx);
        match worktree_on(Some(&others), reference) {
            None => {
                let reference = reference.to_owned();
                self.repository.update(cx, |repository, cx| {
                    drop(repository.switch_branch(reference, cx))
                });
            }
            Some(worktree) if can_open_worktree(worktree) => {
                match BranchMenuProvider::current(cx) {
                    Some(provider) => {
                        let path = worktree.path.clone();
                        provider.update(cx, |provider, cx| provider.open_worktree(path, cx));
                    }
                    None => WorktreeMenu::open(self.repository.entity(), worktree, cx),
                }
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
        let poppable = can_pop(
            shown.status().data().map(|status| &status.counts),
            stashes.map(|stashes| &stashes[..]),
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

    fn row_context(&self, cx: &mut Context<Self>) -> Rc<RowContext> {
        Rc::new(RowContext {
            list: cx.entity().downgrade(),
            other_worktrees: self.other_worktrees(cx),
            branch_menu_for: BranchMenuProvider::current(cx)
                .and_then(|provider| provider.read(cx).open_for().cloned()),
            stash_menu_for: self.stash_menu.open_for().cloned(),
            worktree_menu_for: self.worktree_menu.open_for().cloned(),
            now: gitto_app_data::state::clock::now_ms(),
        })
    }

    /// The section `id` of the list, collapsible under that id, counting `count` items in
    /// `items` rows.
    fn section(
        &self,
        id: &'static str,
        count: usize,
        items: usize,
        render_rows: RenderRows,
        cx: &mut Context<Self>,
    ) -> SidebarSection {
        let (title, icon) = match id {
            LOCAL => ("Local branches", IconName::GitBranch),
            REMOTES => ("Remotes", IconName::Cloud),
            TAGS => ("Tags", IconName::Tag),
            STASHES => ("Stashes", IconName::Inbox),
            _ => ("Worktrees", IconName::FolderGit2),
        };
        let list = cx.entity().downgrade();
        let scroll = SECTIONS
            .iter()
            .position(|section| *section == id)
            .map(|ix| &self.scrolls[ix]);
        SidebarSection::new(id, title, icon, count, items, render_rows)
            .collapsed(self.collapsed.is_collapsed(id))
            .busy(self.is_section_busy(id, cx))
            .when_some(scroll, |section, scroll| section.track_scroll(scroll))
            .on_toggle(move |_, cx| {
                let _ = list.update(cx, |list, cx| list.toggle(id, cx));
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
            let path = path.clone();
            SidebarFolder::new(
                SharedString::from(format!("folder-{path}")),
                name.clone(),
                *count,
                *depth,
                *collapsed,
            )
            .on_toggle(move |_, cx| {
                let path = path.clone();
                let _ = list.update(cx, |list, cx| list.toggle(&path, cx));
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

/// A local or remote branch's line in its tree, with its menu (see `branch_menu`). One checked
/// out in another worktree has a folder after its name, and says which in its tooltip.
fn branch_row(branch: &Ref, label: &SharedString, depth: usize, rows: &RowContext) -> AnyElement {
    let full_name = SharedString::from(branch.full_name.clone());
    let worktree = worktree_on(Some(&rows.other_worktrees), &branch.full_name);
    let tag = worktree.map(|worktree| RowTag {
        icon: IconName::FolderGit2,
        label: format!("Checked out in {}", worktree.name).into(),
    });
    let list = rows.list.clone();
    let activated = full_name.clone();
    let menu_of = full_name.clone();
    SidebarRow::new(
        SharedString::from(format!("branch-{full_name}")),
        IconName::GitBranch,
        label.clone(),
    )
    .title(branch_title(branch, worktree))
    .depth(depth)
    .active(branch.current)
    .highlighted(rows.branch_menu_for.as_ref() == Some(&full_name))
    .tag(tag)
    .meta(branch_meta(branch).map(Into::into))
    .on_double_click(move |_, cx| {
        let _ = list.update(cx, |list, cx| list.activate_branch(&activated, cx));
    })
    .menu(move |menu, window, cx| branch_menu(menu, menu_of.clone(), window, cx))
    .into_any_element()
}

impl Render for RefList {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rail = f32::from(window.viewport_size().width) < MEDIUM;
        self.refresh_trees(cx);
        let rows = self.row_context(cx);
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
                .message(error.message().to_owned())
                .fill(),
            )
        });
        let list = v_flex()
            .id("ref-list")
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
                LOCAL,
                self.trees.local_count,
                self.trees.local.len(),
                tree_rows(Rc::clone(&self.trees.local), Rc::clone(&rows)),
                cx,
            )
            .rail(rail);
        let remotes = self
            .section(
                REMOTES,
                self.trees.remote_count,
                self.trees.remote.len(),
                tree_rows(Rc::clone(&self.trees.remote), Rc::clone(&rows)),
                cx,
            )
            .rail(rail);
        let tags = Rc::clone(&self.trees.tags);
        let tags = self
            .section(
                TAGS,
                tags.len(),
                tags.len(),
                Rc::new(move |range: Range<usize>, _, _| {
                    in_view(&tags, range)
                        .iter()
                        .map(|tag| {
                            SidebarRow::new(
                                SharedString::from(format!("tag-{}", tag.full_name)),
                                IconName::Tag,
                                tag.name.clone(),
                            )
                            .into_any_element()
                        })
                        .collect()
                }),
                cx,
            )
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
            .section(STASHES, stashes.len(), stashes.len(), stash_rows, cx)
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
                WORKTREES,
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
    SidebarRow::new(
        SharedString::from(format!("stash-{sha}")),
        IconName::Archive,
        stash.message.clone(),
    )
    .title(stash_title(stash, rows.now))
    .highlighted(rows.stash_menu_for.as_ref() == Some(&sha))
    .menu(move |menu, _, cx| RefList::stash_menu(&list, menu_of.clone(), menu, cx))
    .into_any_element()
}

/// A worktree's line, with its menu: the one on show active, one whose folder is gone faded.
fn worktree_row(worktree: &Worktree, rows: &RowContext) -> AnyElement {
    let path = SharedString::from(worktree.path.clone());
    let list = rows.list.clone();
    let opened = list.clone();
    let menu_of = path.clone();
    let activated = path.clone();
    SidebarRow::new(
        SharedString::from(format!("worktree-{path}")),
        IconName::FolderGit2,
        worktree.name.clone(),
    )
    .title(describe_worktree(worktree))
    .active(worktree.current)
    .highlighted(rows.worktree_menu_for.as_ref() == Some(&path))
    .faded(worktree.prunable.is_some())
    .meta(Some(worktree_meta(worktree).into()))
    // The section shows it running, and why it failed.
    .on_double_click(move |_, cx| {
        let _ = opened.update(cx, |list, cx| list.activate_worktree(&activated, cx));
    })
    .menu(move |menu, _, cx| RefList::worktree_menu(&list, menu_of.clone(), menu, cx))
    .into_any_element()
}

#[cfg(test)]
mod tests;
