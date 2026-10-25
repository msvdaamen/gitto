//! The uncommitted changes' details. Ported from
//! `components/repository/details/working-tree-details.tsx`.

use std::rc::Rc;

use gitto_git::features::staging::discard::DiscardSide;
use gitto_git::features::status::schema::{StatusSummary, Uncommitted};
use gitto_git::input::RepositoryId;
use gitto_ui_logic::git::changes::is_nested_repository;
use gitto_ui_logic::git::conflicts::operation_name;
use gitto_ui_logic::git::status::head_label;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Context, Entity, EventEmitter, FontWeight, HighlightStyle, SharedString,
    StyledText, Subscription, TestSupportExt as _, Window, div, rems,
};

use super::changed_file_list::{ChangedFileList, FileAction, HoverIntent};
use super::commit_details::{CommitDetailsEvent, ShownFile, shown_path_in};
use super::commit_form::CommitForm;
use super::details_sections::DetailsError;
use super::discard_dialog::{DiscardPrompt, open_discard_dialog};
use super::file_menu::file_menu;
use gitto_app_data::data::diff::{WorkingTreeLists, select_working_tree_lists};
use gitto_app_data::data::staging::{
    DiscardTarget, StagingTarget, discard_mutation, stage_mutation, unstage_mutation,
};
use gitto_app_data::data::status::head_sha;
use gitto_app_data::data::{Changed, DataError, Mutation, Opaque, Part, Shown};
use gitto_app_data::state::delayed::{DELAY, Delayed};
use gitto_app_kit::theme::PaletteExt as _;
use gitto_app_kit::ui::{
    Badge, IconButton, IconName, LinkButton, Mascot, SectionHeader, Tone, UpdatingIndicator,
};
use gitto_ui_logic::git::changes::staging_paths;
use gitto_ui_logic::git::diff_source::DiffSource;

/// Splits the status into the uncommitted changes' lists.
type SelectLists = Box<dyn Fn(&Opaque<Uncommitted>) -> WorkingTreeLists>;

type Staging = Entity<Mutation<(RepositoryId, StagingTarget)>>;

/// What the uncommitted changes' details show of the repository.
const PARTS: &[Part] = &[Part::Status, Part::Operation];

/// Which of the two lists: what isn't staged, and what is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Unstaged,
    Staged,
}

impl Side {
    fn source(self) -> DiffSource {
        match self {
            Side::Unstaged => DiffSource::Unstaged,
            Side::Staged => DiffSource::Staged,
        }
    }

    fn discard_side(self) -> DiscardSide {
        match self {
            Side::Unstaged => DiscardSide::Unstaged,
            Side::Staged => DiscardSide::Staged,
        }
    }

    /// Marks the list, e.g. for tests to find its rows.
    fn list_id(self) -> &'static str {
        match self {
            Side::Unstaged => "unstaged-files",
            Side::Staged => "staged-files",
        }
    }
}

/// The uncommitted changes: what's staged for the next commit, what isn't, and the commit form.
/// The two file lists split the space evenly and scroll on their own, so moving files between
/// them doesn't shift the layout. Right-clicking a file opens a menu of what can be done with it,
/// like discarding its changes; all of them can be discarded at once too. Either asks first.
///
/// There's no Suspense to take it off the page while the status reloads (whenever a file is
/// saved): it stays, its lists scrolled where they were, and says it's updating.
pub struct WorkingTreeDetails {
    repository: Shown,
    /// The uncommitted changes' lists, of the status.
    select_lists: SelectLists,
    stage: Staging,
    unstage: Staging,
    discard: Entity<Mutation<(RepositoryId, DiscardTarget)>>,
    /// The changes being asked about discarding, as they were when that was asked.
    discarding: Option<DiscardTarget>,
    form: Entity<CommitForm>,
    /// The status is reloaded whenever a file changes, which takes a while in a big repository.
    updating: Entity<Delayed>,
    /// The file whose menu is open, in each list.
    menu_for: [Option<String>; 2],
    hover: [Entity<HoverIntent>; 2],
    shown_file: ShownFile,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<CommitDetailsEvent> for WorkingTreeDetails {}

impl WorkingTreeDetails {
    pub fn new(repository_id: &RepositoryId, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let repository = Shown::new(repository_id, PARTS, cx);
        let stage = stage_mutation(cx);
        let unstage = unstage_mutation(cx);
        let discard = discard_mutation(cx);
        let form = cx.new(|cx| CommitForm::new(repository_id.clone(), window, cx));
        let updating = cx.new(|cx| Delayed::new(false, DELAY, cx));
        let subscriptions = vec![
            cx.observe(&stage, |_, _, cx| cx.notify()),
            cx.observe(&unstage, |_, _, cx| cx.notify()),
            cx.observe(&discard, |_, _, cx| cx.notify()),
            cx.observe(&updating, |_, _, cx| cx.notify()),
            cx.subscribe_in(
                repository.entity(),
                window,
                |this, _, Changed(part), window, cx| {
                    if *part == Part::Status {
                        this.status_changed(window, cx)
                    }
                },
            ),
        ];
        let mut details = WorkingTreeDetails {
            repository,
            select_lists: Box::new(select_working_tree_lists()),
            stage,
            unstage,
            discard,
            discarding: None,
            form,
            updating,
            menu_for: [None, None],
            hover: [
                cx.new(|_| HoverIntent::default()),
                cx.new(|_| HoverIntent::default()),
            ],
            shown_file: None,
            _subscriptions: subscriptions,
        };
        details.status_changed(window, cx);
        details
    }

    pub fn set_shown_file(&mut self, shown: ShownFile, cx: &mut Context<Self>) {
        if self.shown_file != shown {
            self.shown_file = shown;
            cx.notify();
        }
    }

    /// The uncommitted changes' lists, once loaded.
    fn lists(&self, cx: &App) -> Option<WorkingTreeLists> {
        let status = self.repository.read(cx).status().data()?;
        Some((self.select_lists)(status))
    }

    fn summary(&self, cx: &App) -> Option<StatusSummary> {
        let status = self.repository.read(cx).status().data()?;
        Some(status.summary())
    }

    /// Follows the status as it reloads: tells the form what's staged and where HEAD is, says
    /// it's updating once that's taken a while, and stops asking about a file whose changes are
    /// gone.
    fn status_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let summary = self.summary(cx);
        let staged = self.lists(cx).map_or(0, |lists| lists.staged().len());
        let last_commit = summary
            .as_ref()
            .and_then(|summary| head_sha(&summary.head).map(str::to_owned));
        let repository_id = self.repository.id(cx).clone();
        self.form.update(cx, |form, cx| {
            form.set_head(repository_id, staged, last_commit, cx)
        });
        let refetching = self.repository.read(cx).status().is_refetching();
        self.updating
            .update(cx, |updating, cx| updating.set(refetching, cx));
        // Gone, it isn't asked about again, should it come back.
        if self.discarding.is_some() && self.discard_prompt(cx).is_none() {
            self.discarding = None;
            window.close_dialog(cx);
        }
        cx.notify();
    }

    fn is_busy(&self, cx: &App) -> bool {
        self.stage.read(cx).is_pending()
            || self.unstage.read(cx).is_pending()
            || self.discard.read(cx).is_pending()
    }

    /// Runs a staging change, forgetting why any of them last failed: what's shown is why the
    /// latest one did, if it did.
    fn stage_files(&mut self, side: Side, target: StagingTarget, cx: &mut Context<Self>) {
        self.reset_failures(cx);
        let input = (self.repository.id(cx).clone(), target);
        let mutation = match side {
            Side::Unstaged => &self.stage,
            Side::Staged => &self.unstage,
        };
        drop(mutation.update(cx, |mutation, cx| mutation.mutate(input, cx)));
    }

    fn discard_changes(&mut self, target: DiscardTarget, cx: &mut Context<Self>) {
        self.reset_failures(cx);
        let input = (self.repository.id(cx).clone(), target);
        drop(
            self.discard
                .update(cx, |mutation, cx| mutation.mutate(input, cx)),
        );
    }

    fn reset_failures(&mut self, cx: &mut Context<Self>) {
        self.stage.update(cx, |mutation, cx| mutation.reset(cx));
        self.unstage.update(cx, |mutation, cx| mutation.reset(cx));
        self.discard.update(cx, |mutation, cx| mutation.reset(cx));
    }

    /// Why all the changes can't be discarded, if they can't.
    fn discard_all_blocker(&self, cx: &App) -> Option<String> {
        // Its commit would finish it without its changes.
        if let Some(operation) = self
            .repository
            .read(cx)
            .operation()
            .data()
            .cloned()
            .flatten()
        {
            return Some(format!(
                "Finish or abort {} before discarding all changes",
                operation_name(&operation)
            ));
        }
        if self
            .summary(cx)
            .is_some_and(|summary| summary.counts.conflicted > 0)
        {
            return Some("Resolve the conflicts before discarding all changes".to_owned());
        }
        // Repositories inside this one are kept.
        let lists = self.lists(cx);
        let staged = lists.as_ref().map_or(0, |lists| lists.staged().len());
        let only_nested = lists
            .as_ref()
            .is_none_or(|lists| lists.unstaged().iter().all(is_nested_repository));
        if staged == 0 && only_nested {
            return Some("No changes to discard".to_owned());
        }
        None
    }

    /// The changes being asked about discarding as their list has them now, which the dialog
    /// asks about: what discarding them does can change with the file, as it's staged in a
    /// terminal, say. None once it's gone.
    fn discard_prompt(&self, cx: &App) -> Option<DiscardPrompt> {
        match self.discarding.as_ref()? {
            DiscardTarget::All => Some(DiscardPrompt::All),
            DiscardTarget::File { side, file } => {
                let lists = self.lists(cx)?;
                let list = match side {
                    DiscardSide::Staged => lists.staged(),
                    DiscardSide::Unstaged => lists.unstaged(),
                };
                let file = list.iter().find(|listed| listed.path == file.path)?.clone();
                // Its unstaged changes discarded, it's put back as it's staged, by either path
                // of a rename.
                let also_staged = lists.staged().iter().any(|staged| {
                    staged.path == file.path || Some(&staged.path) == file.orig_path.as_ref()
                });
                Some(DiscardPrompt::File {
                    side: *side,
                    file,
                    also_staged,
                })
            }
        }
    }

    /// Asks about discarding `target`, and discards it once Discard is chosen.
    fn ask_to_discard(
        &mut self,
        target: DiscardTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.discarding = Some(target);
        cx.notify();
        let this = cx.entity().downgrade();
        let cancel = this.clone();
        let discard = this.clone();
        open_discard_dialog(
            move |cx| this.upgrade()?.read(cx).discard_prompt(cx),
            move |_, cx| {
                let _ = cancel.update(cx, |this, cx| {
                    this.discarding = None;
                    cx.notify();
                });
            },
            move |prompt, _, cx| {
                let _ = discard.update(cx, |this, cx| {
                    this.discarding = None;
                    this.discard_changes(prompt.target(), cx);
                });
            },
            window,
            cx,
        );
    }

    /// The error to show, if any: why the changes couldn't be loaded, or why the latest change to
    /// them failed.
    fn error(&self, cx: &App) -> Option<DataError> {
        self.repository
            .read(cx)
            .status()
            .error()
            .cloned()
            .or_else(|| self.stage.read(cx).error().cloned())
            .or_else(|| self.unstage.read(cx).error().cloned())
            .or_else(|| self.discard.read(cx).error().cloned())
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = *cx.palette();
        let lists = self.lists(cx);
        let (staged, unstaged) = lists.as_ref().map_or((0, 0), |lists| {
            (lists.staged().len(), lists.unstaged().len())
        });
        let branch = self
            .summary(cx)
            .map(|summary| head_label(&summary.head).to_owned());
        let blocker = self.discard_all_blocker(cx);
        let busy = self.is_busy(cx);
        h_flex()
            .flex_none()
            .items_start()
            .gap(rems(0.867))
            .p_4()
            .border_b_1()
            .border_color(palette.border)
            .child(
                div()
                    .self_center()
                    .rounded_full()
                    .shadow_md()
                    .child(Mascot::new(rems(3.467))),
            )
            .child(
                v_flex()
                    .min_w_0()
                    .flex_1()
                    .items_start()
                    .child(Badge::new("WIP").tone(Tone::Amber))
                    .child(
                        div()
                            .mt(rems(0.467))
                            .mb_1p5()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette.text)
                            .child("Uncommitted changes"),
                    )
                    .child({
                        let branch = branch.unwrap_or_default();
                        let text = format!("On {branch} · {staged} staged, {unstaged} unstaged");
                        let bold = HighlightStyle {
                            font_weight: Some(FontWeight::BOLD),
                            color: Some(palette.text_soft),
                            ..Default::default()
                        };
                        div()
                            .id("working-tree-counts")
                            .text_xs()
                            .line_height(rems(1.24))
                            .text_color(palette.muted)
                            .aria_label(SharedString::from(text.clone()))
                            .child(
                                StyledText::new(text)
                                    .with_highlights([(3..3 + branch.len(), bold)]),
                            )
                            .test_support()
                    })
                    .when(self.updating.read(cx).is_on(), |this| {
                        this.child(div().mt_1().child(UpdatingIndicator::new("updating")))
                    }),
            )
            .child({
                let label = blocker
                    .clone()
                    .unwrap_or_else(|| "Discard all changes…".to_owned());
                IconButton::new("discard-all", IconName::Undo2, label)
                    .disabled(busy || blocker.is_some())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.ask_to_discard(DiscardTarget::All, window, cx)
                    }))
            })
    }

    fn render_section(&self, side: Side, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = *cx.palette();
        let lists = self.lists(cx);
        let busy = self.is_busy(cx);
        let ix = side as usize;
        let (title, icon, empty, bulk_label) = match side {
            Side::Unstaged => (
                "Unstaged changes",
                IconName::FilePen,
                "Nothing left to stage.",
                "Stage all",
            ),
            Side::Staged => (
                "Staged changes",
                IconName::CircleCheck,
                "Stage files to include them in the next commit.",
                "Unstage all",
            ),
        };
        let files = lists
            .as_ref()
            .map(|lists| match side {
                Side::Unstaged => lists.unstaged().clone(),
                Side::Staged => lists.staged().clone(),
            })
            .unwrap_or_else(|| Opaque::new(Vec::new()));
        let uncounted = lists.as_ref().is_some_and(|lists| lists.uncounted());
        let count = files.len();
        let source = side.source();

        let this = cx.entity().downgrade();
        let action = {
            let this = this.clone();
            let (label, icon) = match side {
                Side::Unstaged => ("Stage", IconName::Plus),
                Side::Staged => ("Unstage", IconName::Minus),
            };
            FileAction::new(label, icon, move |file, _, cx| {
                let paths = staging_paths(std::slice::from_ref(file));
                let _ = this.update(cx, |this, cx| {
                    this.stage_files(side, StagingTarget::Paths(paths), cx)
                });
            })
            .disabled(busy)
        };
        let mut list = ChangedFileList::new(files)
            .uncounted(uncounted)
            .action(action)
            .highlighted_path(self.menu_for[ix].clone())
            .open_path(shown_path_in(&self.shown_file, &source))
            .menu({
                let this = this.clone();
                move |menu, file, _, cx| {
                    let open_for = this.clone();
                    let discard = this.clone();
                    file_menu(
                        menu,
                        file,
                        busy,
                        Rc::new(move |path, cx| {
                            let _ = open_for.update(cx, |this, cx| {
                                this.menu_for[ix] = path;
                                cx.notify();
                            });
                        }),
                        Rc::new(move |file, window, cx| {
                            let target = DiscardTarget::File {
                                side: side.discard_side(),
                                file: file.clone(),
                            };
                            let _ = discard
                                .update(cx, |this, cx| this.ask_to_discard(target, window, cx));
                        }),
                        cx,
                    )
                }
            })
            .on_open({
                let this = this.clone();
                let source = source.clone();
                move |file, _, cx| {
                    let event = CommitDetailsEvent::OpenFile {
                        source: source.clone(),
                        file: file.clone(),
                    };
                    let _ = this.update(cx, |_, cx| cx.emit(event));
                }
            })
            .on_prefetch(&self.hover[ix], {
                let this = this.clone();
                move |file, _, cx| {
                    let event = CommitDetailsEvent::PrefetchFile {
                        source: source.clone(),
                        file: file.clone(),
                        uncounted,
                    };
                    let _ = this.update(cx, |_, cx| cx.emit(event));
                }
            });
        if side == Side::Unstaged
            && let Some(lists) = lists.as_ref()
        {
            list = list.marker_free(Opaque::new(lists.marker_free().clone()));
        }

        let header =
            SectionHeader::new(icon, title)
                .count(count as u64)
                .when(side == Side::Staged, SectionHeader::mint)
                .child(
                    LinkButton::new(
                        SharedString::from(format!("bulk-{}", side.list_id())),
                        bulk_label,
                    )
                    .disabled(busy || count == 0)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.stage_files(side, StagingTarget::All, cx)
                    })),
                );
        let content: AnyElement = if count == 0 {
            div()
                .px_1()
                .pb_1()
                .text_xs()
                .text_color(palette.faint)
                .child(empty)
                .into_any_element()
        } else {
            list.virtual_list(SharedString::from(format!("{}-rows", side.list_id())))
                .into_any_element()
        };
        v_flex()
            .id(side.list_id())
            .flex_1()
            .flex_basis(rems(0.))
            .min_h(rems(8.667))
            .pt_3()
            .border_b_1()
            .border_color(palette.border)
            .child(div().px(rems(0.933)).pb_2().child(header))
            .child(div().min_h_0().flex_1().px_2p5().pb_3().child(content))
    }
}

impl Render for WorkingTreeDetails {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("working-tree")
            .size_full()
            .min_h_0()
            // Scrolls once the lists are as short as they go, so the commit form is still
            // reachable in a short window.
            .overflow_y_scroll()
            .child(self.render_header(cx))
            .child(DetailsError::new(
                "working-tree-error",
                "Something went wrong",
                self.error(cx),
            ))
            .child(self.render_section(Side::Unstaged, cx))
            .child(self.render_section(Side::Staged, cx))
            .child(self.form.clone())
    }
}

#[cfg(test)]
mod tests;
