//! The selected row's details. Ported from `components/repository/details/commit-details.tsx`.

use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::rc::Rc;
use std::time::Duration;

use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::features::history::schema::Commit;
use gitto_git::input::RepositoryId;
use gitto_ui::git::rows::RowId;
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClipboardItem, Context, Entity, EventEmitter, FontWeight, Role, Subscription,
    Task, TestSupportExt as _, WeakEntity, Window, div, rems,
};

use super::changed_file_list::{ChangedFileList, HoverIntent};
use super::details_data::DetailsData;
use super::details_sections::{DetailsError, DetailsList, FileTotals, RenderTop};
use super::stash_details::StashDetails;
use super::working_tree_details::WorkingTreeDetails;
use crate::components::ui::{
    Avatar, AvatarSize, EmptyState, Icon, IconButton, IconName, LoadingState,
};
use crate::data::diff::{ChangedFiles, FilesOf, ShownFiles, select_shown_files};
use crate::data::{Opaque, Query};
use crate::git::diff_source::DiffSource;
use crate::state::clock::{Clock, relative_time};
use crate::theme::{PaletteExt as _, base_px};
use crate::util::format::{avatar_color, initials};

/// What the details ask of the page (TS's `FileOpener`); each part of the details emits them too,
/// and the details pass them on.
#[derive(Debug, Clone, PartialEq)]
pub enum CommitDetailsEvent {
    /// Shows the changes in a file of one of the lists, in place of the history.
    OpenFile {
        source: DiffSource,
        file: ChangedFile,
    },
    /// Loads a file's changes ahead, as it's likely to be opened (`prefetchFileDiff`).
    PrefetchFile {
        source: DiffSource,
        file: ChangedFile,
        uncounted: bool,
    },
}

/// The file whose changes are on show, by its source and path.
pub type ShownFile = Option<(DiffSource, String)>;

/// The path of the file on show, if it's one of `source`'s, for its list to mark it
/// (`shownPathIn`).
pub fn shown_path_in(shown: &ShownFile, source: &DiffSource) -> Option<String> {
    shown
        .as_ref()
        .filter(|(shown, _)| shown.is_same(source))
        .map(|(_, path)| path.clone())
}

/// `list`, whose files open, and load ahead once the pointer rests on one, as `source`'s, through
/// `owner`'s events; the file on show marked (TS's `ChangedFilesSection`, given an opener). Their
/// lines are counted, unlike the working tree's.
pub fn openable_files<T: EventEmitter<CommitDetailsEvent>>(
    list: ChangedFileList,
    source: DiffSource,
    shown_file: &ShownFile,
    hover: &Entity<HoverIntent>,
    cx: &Context<T>,
) -> ChangedFileList {
    let opener = cx.entity().downgrade();
    let prefetcher = opener.clone();
    let prefetch_source = source.clone();
    list.open_path(shown_path_in(shown_file, &source))
        .on_open(move |file, _, cx| {
            let event = CommitDetailsEvent::OpenFile {
                source: source.clone(),
                file: file.clone(),
            };
            let _ = opener.update(cx, |_, cx| cx.emit(event));
        })
        .on_prefetch(hover, move |file, _, cx| {
            let event = CommitDetailsEvent::PrefetchFile {
                source: prefetch_source.clone(),
                file: file.clone(),
                uncounted: false,
            };
            let _ = prefetcher.update(cx, |_, cx| cx.emit(event));
        })
}

/// What the details show: the part for the kind of row selected.
enum Body {
    Commit(Entity<SelectedCommit>),
    Stash(Entity<StashDetails>),
    WorkingTree(Entity<WorkingTreeDetails>),
}

/// Details of the selected history row: a commit, a stash, or the uncommitted changes (with the
/// commit form). Loads what it shows by itself, so it only needs the row's id from the history.
/// An entity: the page tells it the row ([`set_selected`](Self::set_selected)) and the file on
/// show, for its lists to mark ([`set_shown_file`](Self::set_shown_file)), and handles its
/// [`CommitDetailsEvent`]s.
pub struct CommitDetails {
    repository_id: RepositoryId,
    selected_id: Option<RowId>,
    shown_file: ShownFile,
    data: DetailsData,
    /// The part showing the selected row, of the repository it was selected in: switching
    /// repositories clears the selection until the history picks a row, and what's on show is
    /// never asked for another repository's commit.
    body: Option<(RepositoryId, Body)>,
    _body_events: Option<Subscription>,
}

impl EventEmitter<CommitDetailsEvent> for CommitDetails {}

impl CommitDetails {
    pub fn new(repository_id: RepositoryId, window: &mut Window, cx: &mut Context<Self>) -> Self {
        CommitDetails::with_data(repository_id, DetailsData::git(), window, cx)
    }

    /// The details, loading and changing what they show through `data` (a test's, say).
    pub fn with_data(
        repository_id: RepositoryId,
        data: DetailsData,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Self {
        CommitDetails {
            repository_id,
            selected_id: None,
            shown_file: None,
            data,
            body: None,
            _body_events: None,
        }
    }

    pub fn repository_id(&self) -> &RepositoryId {
        &self.repository_id
    }

    /// Shows the repository `repository_id`'s rows from now on; what's on show stays until another
    /// row is selected.
    pub fn set_repository(&mut self, repository_id: RepositoryId, cx: &mut Context<Self>) {
        if self.repository_id != repository_id {
            self.repository_id = repository_id;
            cx.notify();
        }
    }

    /// The row the details are of; `None` shows none.
    pub fn set_selected(
        &mut self,
        selected_id: Option<RowId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_id == selected_id {
            return;
        }
        self.selected_id = selected_id;
        self.show_selected(window, cx);
        cx.notify();
    }

    /// The file whose changes are on show, if any, by its source and path: the part on show
    /// marks it, and the details themselves show nothing of it.
    pub fn set_shown_file(&mut self, shown: ShownFile, cx: &mut Context<Self>) {
        if self.shown_file == shown {
            return;
        }
        self.shown_file = shown.clone();
        match self.body.as_ref().map(|(_, body)| body) {
            Some(Body::Commit(commit)) => {
                commit.update(cx, |commit, cx| commit.set_shown_file(shown, cx))
            }
            Some(Body::Stash(stash)) => {
                stash.update(cx, |stash, cx| stash.set_shown_file(shown, cx))
            }
            Some(Body::WorkingTree(tree)) => {
                tree.update(cx, |tree, cx| tree.set_shown_file(shown, cx))
            }
            None => {}
        }
    }

    /// Brings the part on show in line with the selected row: the same part, told the commit or
    /// stash, if it's of the same kind and repository; else a new one.
    fn show_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected_id.clone() else {
            self.body = None;
            self._body_events = None;
            return;
        };
        let repository_id = self.repository_id.clone();
        let same_repository = self
            .body
            .as_ref()
            .is_some_and(|(of, _)| *of == repository_id);
        let shown = self.shown_file.clone();
        let data = self.data.clone();
        match id {
            RowId::Wip => {
                if same_repository && matches!(self.body, Some((_, Body::WorkingTree(_)))) {
                    return;
                }
                let tree = cx.new(|cx| {
                    let mut tree = WorkingTreeDetails::new(repository_id.clone(), data, window, cx);
                    tree.set_shown_file(shown, cx);
                    tree
                });
                self.adopt(repository_id, tree, Body::WorkingTree, cx);
            }
            RowId::Stash(sha) => {
                if let Some((_, Body::Stash(stash))) =
                    self.body.as_ref().filter(|_| same_repository)
                {
                    stash.update(cx, |stash, cx| stash.set_sha(&sha, cx));
                    return;
                }
                let stash = cx.new(|cx| {
                    let mut stash = StashDetails::new(repository_id.clone(), &sha, data, cx);
                    stash.set_shown_file(shown, cx);
                    stash
                });
                self.adopt(repository_id, stash, Body::Stash, cx);
            }
            RowId::Commit(id) => {
                if let Some((_, Body::Commit(commit))) =
                    self.body.as_ref().filter(|_| same_repository)
                {
                    commit.update(cx, |commit, cx| commit.set_sha(&id, cx));
                    return;
                }
                let commit = cx.new(|cx| {
                    let mut commit = SelectedCommit::new(repository_id.clone(), &id, data, cx);
                    commit.set_shown_file(shown, cx);
                    commit
                });
                self.adopt(repository_id, commit, Body::Commit, cx);
            }
        }
    }

    /// Shows `part`, of `repository_id`, passing its events on.
    fn adopt<T: EventEmitter<CommitDetailsEvent>>(
        &mut self,
        repository_id: RepositoryId,
        part: Entity<T>,
        body: fn(Entity<T>) -> Body,
        cx: &mut Context<Self>,
    ) {
        self._body_events = Some(cx.subscribe(&part, |_, _, event: &CommitDetailsEvent, cx| {
            cx.emit(event.clone())
        }));
        self.body = Some((repository_id, body(part)));
    }

    fn title(&self) -> &'static str {
        match self.body.as_ref().map(|(_, body)| body) {
            Some(Body::WorkingTree(_)) => "Working directory",
            Some(Body::Stash(_)) => "Stash",
            _ => "Commit details",
        }
    }
}

impl Render for CommitDetails {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = *cx.palette();
        let Some((_, body)) = self.body.as_ref().filter(|_| self.selected_id.is_some()) else {
            return v_flex()
                .id("details")
                .size_full()
                .child(
                    EmptyState::new(
                        "nothing-selected",
                        IconName::GitCommitHorizontal,
                        "Nothing selected",
                    )
                    .fill(),
                )
                .into_any_element();
        };
        let body: AnyElement = match body {
            Body::Commit(commit) => commit.clone().into_any_element(),
            Body::Stash(stash) => stash.clone().into_any_element(),
            Body::WorkingTree(tree) => tree.clone().into_any_element(),
        };
        v_flex()
            .id("details")
            .size_full()
            .min_w(base_px(280.))
            .child(
                h_flex()
                    .flex_none()
                    .h(base_px(38.))
                    .justify_between()
                    .pl(base_px(13.))
                    .pr(base_px(9.))
                    .border_b_1()
                    .border_color(palette.border)
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::EXTRA_BOLD)
                            .text_color(palette.muted)
                            .child(self.title().to_uppercase()),
                    )
                    // Nothing to do yet: not one to focus and press.
                    .child(
                        IconButton::new(
                            "more-commit-actions",
                            IconName::Ellipsis,
                            "More commit actions",
                        )
                        .disabled(true),
                    ),
            )
            .child(div().flex_1().min_h_0().child(body))
            .into_any_element()
    }
}

/// How long the copy button says the SHA was copied.
const COPIED: Duration = Duration::from_millis(1500);

/// A commit selected in the history: who made it and when, its message, its SHA (copied with a
/// click), and the files it changed, scrolled with the message.
pub struct SelectedCommit {
    repository_id: RepositoryId,
    sha: String,
    data: DetailsData,
    details: Query<Commit>,
    changes: Query<ChangedFiles, ShownFiles>,
    /// The last commit both it and its files are loaded for. The commit and its files are loaded
    /// apart, and either can arrive first when another is selected: what's on show is the last
    /// commit both are loaded for, so the message is never another commit's than the files. Until
    /// one fails, which leaves the other on its own.
    shown: Option<(Commit, ShownFiles)>,
    shown_file: ShownFile,
    list: DetailsList,
    hover: Entity<HoverIntent>,
    /// Whether the SHA was just copied, until the timer's done.
    copied: Option<Task<()>>,
    _subscriptions: [Subscription; 2],
}

impl EventEmitter<CommitDetailsEvent> for SelectedCommit {}

impl SelectedCommit {
    pub fn new(
        repository_id: RepositoryId,
        sha: &str,
        data: DetailsData,
        cx: &mut Context<Self>,
    ) -> Self {
        let details = Query::new(data.commit(&repository_id, sha, cx), cx);
        let changes = Query::with_select(
            data.changed_files(&repository_id, sha, FilesOf::Commit, cx),
            select_shown_files(),
            cx,
        );
        let clock = Clock::global(cx);
        let mut commit = SelectedCommit {
            repository_id,
            sha: sha.to_owned(),
            data,
            details,
            changes,
            shown: None,
            shown_file: None,
            list: DetailsList::new(),
            hover: cx.new(|_| HoverIntent::default()),
            copied: None,
            _subscriptions: [
                cx.observe(&clock, |_, _, cx| cx.notify()),
                // The commit's queries notify it as they load.
                cx.observe_self(|this, _| this.update_shown()),
            ],
        };
        // Either may be loaded already.
        commit.update_shown();
        commit
    }

    /// Shows the commit `sha` instead; the one on show stays until it's loaded.
    pub fn set_sha(&mut self, sha: &str, cx: &mut Context<Self>) {
        if self.sha == sha {
            return;
        }
        self.sha = sha.to_owned();
        let details = self.data.commit(&self.repository_id, sha, cx);
        self.details.set_options(details, cx);
        let files = self
            .data
            .changed_files(&self.repository_id, sha, FilesOf::Commit, cx);
        self.changes.set_options(files, cx);
        self.copied = None;
        cx.notify();
    }

    pub fn set_shown_file(&mut self, shown: ShownFile, cx: &mut Context<Self>) {
        if self.shown_file != shown {
            self.shown_file = shown;
            cx.notify();
        }
    }

    /// Copies the commit's full SHA to the clipboard, and says so for a moment.
    fn copy_sha(&mut self, sha: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(sha));
        self.copied = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COPIED).await;
            let _ = this.update(cx, |this, cx| {
                this.copied = None;
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Brings what's on show up to date with what's loaded (see `shown`).
    fn update_shown(&mut self) {
        if self.details.error().is_some() || self.changes.error().is_some() {
            self.shown = None;
            return;
        }
        if let (Some(commit), Some(files)) = (self.details.data(), self.changes.data())
            && commit.sha == files.shown_sha
        {
            self.shown = Some((commit, files));
        }
    }
}

impl Render for SelectedCommit {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let error = self.details.error().or_else(|| self.changes.error());
        let files = self
            .shown
            .as_ref()
            .map(|(_, files)| files.clone())
            .or_else(|| self.changes.data());
        let commit = self
            .shown
            .as_ref()
            .map(|(commit, _)| commit.clone())
            .or_else(|| self.changes.error().and_then(|_| self.details.data()));
        let loading = commit.is_none() && error.is_none();
        let source = DiffSource::Commit {
            sha: self.sha.clone(),
        };
        // Only once the files on show are this commit's, so another's aren't opened as its.
        let openable = self
            .shown
            .as_ref()
            .is_some_and(|(commit, _)| commit.sha == self.sha);

        let (count, totals) = files.as_ref().map_or((0, Default::default()), |files| {
            (files.files.len(), files.totals)
        });
        let copied = self.copied.is_some();
        let this = cx.entity().downgrade();
        let top: RenderTop = {
            let error = error.clone();
            let commit = commit.clone();
            Rc::new(move |_, _| {
                v_flex()
                    .w_full()
                    .child(DetailsError::new(
                        "commit-error",
                        "Couldn't load the commit",
                        error.clone(),
                    ))
                    .when(loading, |this| {
                        this.child(LoadingState::new("loading-details", "Loading details…"))
                    })
                    .when_some(commit.clone(), |el, commit| {
                        el.child(CommitSummary {
                            commit,
                            count,
                            totals,
                            copied,
                            owner: this.clone(),
                        })
                    })
                    .into_any_element()
            })
        };
        let mut hasher = DefaultHasher::new();
        (
            commit.as_ref().map(|commit| commit.sha.clone()),
            error.as_ref().map(|error| error.message()),
            loading,
            count,
        )
            .hash(&mut hasher);

        let mut list = ChangedFileList::new(
            files
                .map(|files| files.files)
                .unwrap_or_else(|| Opaque::new(Vec::new())),
        );
        if openable {
            list = openable_files(list, source, &self.shown_file, &self.hover, cx);
        }
        self.list
            .render("commit-files", top, hasher.finish(), list, cx)
    }
}

/// Who made the commit and when, its message, its SHA, and how much it changed.
#[derive(IntoElement)]
struct CommitSummary {
    commit: Commit,
    count: usize,
    totals: crate::git::changes::LineCounts,
    copied: bool,
    owner: WeakEntity<SelectedCommit>,
}

impl RenderOnce for CommitSummary {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        let commit = self.commit;
        let short_sha: String = commit.sha.chars().take(7).collect();
        let sha = commit.sha.clone();
        let owner = self.owner;
        v_flex()
            .p_4()
            .border_b_1()
            .border_color(palette.border)
            .child(
                h_flex()
                    .gap(base_px(9.))
                    .child(
                        Avatar::new(initials(&commit.author_name))
                            .color(avatar_color(&commit.author_email))
                            .size(AvatarSize::Medium),
                    )
                    .child(
                        v_flex()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(palette.text)
                                    .child(commit.author_name.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(palette.faint)
                                    .child(relative_time(commit.committed_at)),
                            ),
                    ),
            )
            .child(
                div()
                    .mt(base_px(14.))
                    .mb_1p5()
                    .text_sm()
                    .line_height(rems(1.25))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(palette.text)
                    .child(commit.subject.clone()),
            )
            // Scrolls on its own past a few lines, so a long one doesn't push the files out of
            // sight.
            .when(!commit.body.is_empty(), |this| {
                this.child(
                    div()
                        .id("description")
                        .max_h(base_px(111.))
                        .overflow_y_scroll()
                        .text_xs()
                        .line_height(rems(1.24))
                        .text_color(palette.muted)
                        .whitespace_normal()
                        .child(commit.body.clone()),
                )
            })
            .child(
                h_flex().mt_3().child(
                    h_flex()
                        .overflow_hidden()
                        .rounded(base_px(5.))
                        .border_1()
                        .border_color(palette.border_soft)
                        .child(
                            div()
                                .px(base_px(7.))
                                .py_1()
                                .bg(palette.bg)
                                .font_family(cx.theme().mono_font_family.clone())
                                .text_xs()
                                .text_color(palette.text_soft)
                                .child(short_sha),
                        )
                        .child(
                            div()
                                .id("copy-sha")
                                .role(Role::Button)
                                .aria_label(if self.copied {
                                    "Copied the commit SHA"
                                } else {
                                    "Copy commit SHA"
                                })
                                .flex()
                                .items_center()
                                .justify_center()
                                .h(base_px(23.))
                                .w(base_px(24.))
                                .border_l_1()
                                .border_color(palette.border_soft)
                                .bg(palette.panel_raised)
                                .text_color(palette.faint)
                                .hover(|style| style.text_color(palette.text_soft))
                                .tooltip({
                                    let copied = self.copied;
                                    move |window, cx| {
                                        gpui_kit::component::tooltip::Tooltip::new(if copied {
                                            "Copied"
                                        } else {
                                            "Copy the full SHA"
                                        })
                                        .build(window, cx)
                                    }
                                })
                                .on_click(move |_, _, cx| {
                                    let sha = sha.clone();
                                    let _ = owner.update(cx, |commit, cx| commit.copy_sha(sha, cx));
                                })
                                .child(if self.copied {
                                    Icon::new(IconName::Check).size_3().text_color(palette.mint)
                                } else {
                                    Icon::new(IconName::Copy).size_3()
                                })
                                .test_support(),
                        ),
                ),
            )
            .child(FileTotals::new(self.count, self.totals))
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use gitto_git::features::stash::schema::Stash;
    use gitto_git::schema::FileStatus;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AnyWindowHandle, SharedString, TestAppContext};

    use super::*;
    use crate::components::repository::details::test_fakes::{Fakes, file, open, repository_id};

    struct Details {
        _dir: tempfile::TempDir,
        window: AnyWindowHandle,
        details: Entity<CommitDetails>,
        fakes: Rc<Fakes>,
        events: Rc<RefCell<Vec<CommitDetailsEvent>>>,
    }

    fn render_details(cx: &mut TestAppContext, fakes: Rc<Fakes>) -> Details {
        let data = fakes.data();
        let (dir, window, details) = open(cx, move |window, cx| {
            CommitDetails::with_data(repository_id(), data, window, cx)
        });
        let events = Rc::new(RefCell::new(Vec::new()));
        cx.update(|cx| {
            let events = Rc::clone(&events);
            cx.subscribe(&details, move |_, event: &CommitDetailsEvent, _| {
                events.borrow_mut().push(event.clone())
            })
            .detach();
        });
        Details {
            _dir: dir,
            window,
            details,
            fakes,
            events,
        }
    }

    impl Details {
        fn select(&self, id: Option<RowId>, cx: &mut TestAppContext) {
            let details = self.details.clone();
            cx.update_window(self.window, |_, window, cx| {
                details.update(cx, |details, cx| details.set_selected(id, window, cx))
            })
            .unwrap();
            cx.run_until_parked();
        }

        fn title(&self, cx: &mut TestAppContext) -> Option<&'static str> {
            self.details.read_with(cx, |details, _| {
                details.selected_id.as_ref().map(|_| details.title())
            })
        }

        /// The commit on show, and whose files are.
        fn shown_commit(&self, cx: &mut TestAppContext) -> Option<(String, String)> {
            self.details
                .read_with(cx, |details, cx| match &details.body {
                    Some((_, Body::Commit(commit))) => commit
                        .read(cx)
                        .shown
                        .as_ref()
                        .map(|(commit, files)| (commit.sha.clone(), files.shown_sha.clone())),
                    _ => None,
                })
        }

        fn click(&self, id: &str, cx: &mut TestAppContext) {
            let id = SharedString::from(id.to_owned());
            cx.update_window(self.window, |_, window, cx| {
                window.render_frame(cx);
                window.click(id, cx);
            })
            .unwrap();
            cx.run_until_parked();
        }
    }

    #[gpui_kit::test]
    fn shows_the_part_for_the_row_selected(cx: &mut TestAppContext) {
        let details = render_details(cx, Fakes::new());
        assert_eq!(details.title(cx), None);

        details.select(Some(RowId::Wip), cx);
        assert_eq!(details.title(cx), Some("Working directory"));
        details.select(Some(RowId::Stash("s1".into())), cx);
        assert_eq!(details.title(cx), Some("Stash"));
        details.select(Some(RowId::Commit("a1".into())), cx);
        assert_eq!(details.title(cx), Some("Commit details"));
        details.select(None, cx);
        assert_eq!(details.title(cx), None);
        assert!(
            details
                .details
                .read_with(cx, |details, _| details.body.is_none())
        );
    }

    #[gpui_kit::test]
    fn keeps_where_the_files_are_scrolled_to_until_another_commits_are_shown(
        cx: &mut TestAppContext,
    ) {
        let fakes = Fakes::new();
        let many = (0..200)
            .map(|ix| file(&format!("{ix}.txt"), FileStatus::Modified, None))
            .collect();
        fakes.files.once(Ok(many));
        let details = render_details(cx, fakes);
        details.select(Some(RowId::Commit("a1".into())), cx);
        let commit = details
            .details
            .read_with(cx, |details, _| match &details.body {
                Some((_, Body::Commit(commit))) => commit.clone(),
                _ => panic!("no commit on show"),
            });
        let state = commit.read_with(cx, |commit, _| commit.list.state().clone());
        let render = |cx: &mut TestAppContext| {
            cx.update_window(details.window, |_, window, cx| window.render_frame(cx))
                .unwrap();
        };
        render(cx);
        state.scroll_to(gpui_kit::ListOffset {
            item_ix: 50,
            offset_in_item: gpui_kit::px(0.),
        });
        render(cx);
        assert_eq!(state.logical_scroll_top().item_ix, 50);

        // Rendered again (a clock tick, a hover): still there.
        commit.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
        render(cx);
        assert_eq!(state.logical_scroll_top().item_ix, 50);

        // Another commit's files still loading: its files aren't on show yet.
        let files = details.fakes.files.later();
        details.select(Some(RowId::Commit("b2".into())), cx);
        render(cx);
        assert_eq!(state.logical_scroll_top().item_ix, 50);

        // They are now, from the top.
        files
            .send(Ok(vec![file("b2.txt", FileStatus::Added, None)]))
            .unwrap();
        cx.run_until_parked();
        render(cx);
        assert_eq!(state.logical_scroll_top().item_ix, 0);
    }

    #[gpui_kit::test]
    fn has_no_commit_actions_to_press_yet(cx: &mut TestAppContext) {
        let details = render_details(cx, Fakes::new());
        details.select(Some(RowId::Commit("a1".into())), cx);
        let disabled = cx
            .update_window(details.window, |_, window, cx| {
                window.render_frame(cx);
                window.find("more-commit-actions").disabled()
            })
            .unwrap();
        assert_eq!(disabled, Some(true));
    }

    /// Holds a view without rendering it.
    struct Unrendered<V: 'static> {
        view: Entity<V>,
    }

    impl<V: 'static> Render for Unrendered<V> {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    #[gpui_kit::test]
    fn takes_a_commit_on_show_once_loaded_without_being_rendered(cx: &mut TestAppContext) {
        let fakes = Fakes::new();
        let data = fakes.data();
        let _dir = crate::test_support::init(cx, |services| services);
        let (_, host) = crate::test_support::open(cx, move |_, cx| Unrendered {
            view: cx.new(|cx| SelectedCommit::new(repository_id(), "a1", data, cx)),
        });
        cx.run_until_parked();
        let commit = host.read_with(cx, |host, _| host.view.clone());
        assert_eq!(
            commit
                .read_with(cx, |commit, _| commit.shown.as_ref().map(
                    |(commit, files)| (commit.sha.clone(), files.shown_sha.clone())
                )),
            Some(("a1".to_owned(), "a1".to_owned()))
        );
    }

    #[gpui_kit::test]
    fn shows_a_commits_message_with_its_own_files_not_anothers(cx: &mut TestAppContext) {
        let details = render_details(cx, Fakes::new());
        details.select(Some(RowId::Commit("a1".into())), cx);
        assert_eq!(
            details.shown_commit(cx),
            Some(("a1".to_owned(), "a1".to_owned()))
        );

        // The next commit comes in before its files: the last one stays on show, and its files
        // aren't opened as the next one's.
        let files = details.fakes.files.later();
        details.select(Some(RowId::Commit("b2".into())), cx);
        assert_eq!(
            details.shown_commit(cx),
            Some(("a1".to_owned(), "a1".to_owned()))
        );
        details.click("file:a1.txt", cx);
        assert!(details.events.borrow().is_empty());

        files
            .send(Ok(vec![file("b2.txt", FileStatus::Added, None)]))
            .unwrap();
        cx.run_until_parked();
        assert_eq!(
            details.shown_commit(cx),
            Some(("b2".to_owned(), "b2".to_owned()))
        );
        details.click("file:b2.txt", cx);
        assert_eq!(
            *details.events.borrow(),
            [CommitDetailsEvent::OpenFile {
                source: DiffSource::Commit { sha: "b2".into() },
                file: file("b2.txt", FileStatus::Added, None),
            }]
        );
    }

    #[gpui_kit::test]
    fn marks_the_file_on_show_in_its_own_list_only(cx: &mut TestAppContext) {
        let details = render_details(cx, Fakes::new());
        details.select(Some(RowId::Commit("a1".into())), cx);
        let shown = |source: DiffSource| Some((source, "a1.txt".to_owned()));
        let set_shown = |shown: ShownFile, cx: &mut TestAppContext| {
            details
                .details
                .update(cx, |details, cx| details.set_shown_file(shown, cx));
        };
        let selected = |cx: &mut TestAppContext| {
            cx.update_window(details.window, |_, window, cx| {
                window.render_frame(cx);
                window.find("file:a1.txt").selected()
            })
            .unwrap()
        };

        set_shown(shown(DiffSource::Commit { sha: "a1".into() }), cx);
        assert_eq!(selected(cx), Some(true));
        // The same path, in the staged changes.
        set_shown(shown(DiffSource::Staged), cx);
        assert_ne!(selected(cx), Some(true));
    }

    #[gpui_kit::test]
    fn names_the_stash_whose_files_are_on_show(cx: &mut TestAppContext) {
        let fakes = Fakes::new();
        *fakes.stashes.borrow_mut() = vec![Stash {
            sha: "s1".into(),
            base: "a1".into(),
            message: "WIP on main: a1 Commit a1".into(),
            created_at: 0,
        }];
        let details = render_details(cx, fakes);
        details.select(Some(RowId::Stash("s1".into())), cx);
        details.click("file:s1.txt", cx);
        assert_eq!(
            *details.events.borrow(),
            [CommitDetailsEvent::OpenFile {
                source: DiffSource::Stash { sha: "s1".into() },
                file: file("s1.txt", FileStatus::Modified, None),
            }]
        );
    }
}
