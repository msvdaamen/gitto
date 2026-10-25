//! What's under way in the repository. Ported from `components/repository/operation-bar.tsx`.

use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::features::operation::schema::{Operation, OperationKind};
use gitto_git::input::RepositoryId;
use gitto_ui_logic::git::conflicts::{operation_name, operation_progress, operation_title};
use gpui_kit::component::button::{ButtonVariant, ButtonVariants as _};
use gpui_kit::component::{Disableable as _, h_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Context, EventEmitter, FontWeight, Role, SharedString, Task, Window, div, rems,
};

use super::BeforeChange;
use gitto_app_data::data::changes::{ABORT, CONTINUE};
use gitto_app_data::data::status::{ConflictedFiles, first_to_resolve, select_conflicted};
use gitto_app_data::data::{Part, Repository, Shown};
use gitto_app_kit::theme::PaletteExt as _;
use gitto_app_kit::ui::{ConfirmDialog, FailurePopover, Icon, IconName, Tone, action_button};

/// What the operation bar asks of the page.
#[derive(Debug, Clone, PartialEq)]
pub enum OperationBarEvent {
    /// Opens a conflicted file's conflicts.
    Resolve(ChangedFile),
}

/// What continuing does, on its button: commits a merge, goes on with the rest.
pub fn continue_label(kind: OperationKind) -> &'static str {
    if kind == OperationKind::Merge {
        "Commit merge"
    } else {
        "Continue"
    }
}

/// How many files have conflicts, and how many of them are ready to mark resolved; once none has,
/// with an operation under way, that it can go on; with neither, nothing.
pub fn conflicts_summary(operation: bool, files: usize, ready: usize) -> String {
    if files == 0 {
        return if operation {
            "All conflicts resolved".to_owned()
        } else {
            String::new()
        };
    }
    let conflicts = format!(
        "{files} {} conflicts",
        if files == 1 { "file has" } else { "files have" }
    );
    if ready > 0 {
        format!("{conflicts}, {ready} ready to mark resolved")
    } else {
        conflicts
    }
}

/// What's under way in the repository, above the history and a file's changes: a merge, a
/// rebase, cherry-picks or reverts, with how many files still have conflicts, and the conflicts a
/// stash left as it was popped. The first conflicted file is a click away; once none is left, the
/// operation is continued, as git would from a terminal. One that goes on to stop at the next
/// commit's conflicts, as a rebase of several commits can, opens the first of those to resolve.
/// Aborting it, which throws away what's been resolved, asks first.
///
/// An entity: it loads the operation and the conflicted files. Continuing or aborting waits for
/// [`set_before_change`](Self::set_before_change)'s guard, as either rewrites the working tree.
pub struct OperationBar {
    repository: Shown,
    before_change: Option<BeforeChange>,
}

impl EventEmitter<OperationBarEvent> for OperationBar {}

impl OperationBar {
    pub fn new(repository_id: RepositoryId, _: &mut Window, cx: &mut Context<Self>) -> Self {
        OperationBar {
            repository: Shown::new(
                &repository_id,
                &[Part::Operation, Part::Status, Part::Operations],
                cx,
            ),
            before_change: None,
        }
    }

    pub fn set_repository(&mut self, repository_id: RepositoryId, cx: &mut Context<Self>) {
        self.repository.set(&repository_id, cx);
    }

    /// What continuing or aborting waits for: the edits to a file on show saved.
    pub fn set_before_change(&mut self, guard: BeforeChange) {
        self.before_change = Some(guard);
    }

    /// The operation under way, if any.
    pub fn operation(&self, cx: &App) -> Option<Operation> {
        self.repository
            .read(cx)
            .operation()
            .data()
            .cloned()
            .flatten()
    }

    fn conflicted(&self, cx: &App) -> Option<ConflictedFiles> {
        conflicted(self.repository.read(cx))
    }

    fn files(&self, cx: &App) -> Vec<ChangedFile> {
        self.conflicted(cx)
            .map(|conflicted| conflicted.files)
            .unwrap_or_default()
    }

    /// Whether it's shown: there's an operation under way, or conflicts.
    pub fn is_shown(&self, cx: &App) -> bool {
        self.operation(cx).is_some() || !self.files(cx).is_empty()
    }

    /// What's said of the conflicts, as the bar shows it.
    pub fn summary(&self, cx: &App) -> String {
        let conflicted = self.conflicted(cx);
        let files = conflicted.as_ref().map_or(0, |c| c.files.len());
        let ready = conflicted.as_ref().map_or(0, |c| c.marker_free.len());
        let operation = self.operation(cx).is_some();
        let summary = conflicts_summary(operation, files, ready);
        if !operation && files > 0 {
            format!("{summary}. Resolve them, then commit.")
        } else {
            summary
        }
    }

    /// The file "Resolve conflicts" opens: one with markers left, if any is.
    pub fn first(&self, cx: &App) -> Option<ChangedFile> {
        self.conflicted(cx)
            .and_then(|conflicted| first_to_resolve(&conflicted).cloned())
    }

    fn is_pending(&self, cx: &App) -> bool {
        self.repository.read(cx).is_running(&[CONTINUE, ABORT])
    }

    /// Whether Continue (or Commit merge) can be clicked: nothing's running, and nothing's
    /// conflicted.
    pub fn may_continue(&self, cx: &App) -> bool {
        !self.is_pending(cx) && self.files(cx).is_empty()
    }

    fn saved(&self, cx: &mut App) -> Task<bool> {
        match &self.before_change {
            Some(before_change) => before_change(cx),
            None => Task::ready(true),
        }
    }

    /// Continues `kind` once the edits to a file on show are saved: it rewrites files. If it
    /// stops at the next commit's conflicts, the first conflicted file is opened to resolve them.
    pub fn go_on(&mut self, kind: OperationKind, cx: &mut Context<Self>) {
        let id = self.repository.id(cx).clone();
        let saved = self.saved(cx);
        cx.spawn(async move |this, cx| {
            if !saved.await {
                return;
            }
            let Ok(Some(done)) = this.update(cx, |this, cx| {
                if this.repository.id(cx) != &id {
                    return None;
                }
                this.repository
                    .update(cx, |repository, cx| repository.continue_operation(kind, cx))
            }) else {
                return;
            };
            if !matches!(done.await, Ok(Ok(()))) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                if this.repository.id(cx) != &id {
                    return;
                }
                // The status has reloaded by now (see `Repository::continue_operation`).
                if let Some(file) = this.first(cx) {
                    cx.emit(OperationBarEvent::Resolve(file));
                }
            });
        })
        .detach();
    }

    /// Aborts `kind` once the edits to a file on show are saved, as continuing does.
    pub fn abort(&mut self, kind: OperationKind, cx: &mut Context<Self>) {
        let repository = self.repository.entity().clone();
        let saved = self.saved(cx);
        cx.spawn(async move |_, cx| {
            if saved.await {
                repository.update(cx, |repository, cx| {
                    drop(repository.abort_operation(kind, cx));
                });
            }
        })
        .detach();
    }

    /// Asks before aborting `operation`, which puts the repository back as it was before it:
    /// what's been resolved so far is lost. Esc, or clicking outside, cancels.
    fn confirm_abort(&mut self, operation: Operation, window: &mut Window, cx: &mut Context<Self>) {
        let name = operation_name(&operation);
        let kind = operation.kind();
        let this = cx.entity();
        let id = self.repository.id(cx).clone();
        ConfirmDialog::new(
            IconName::TriangleAlert,
            Tone::Coral,
            format!("Abort {name}?"),
        )
        .description(format!(
            "This puts the repository back as it was before {name} started. The conflicts \
                 you've resolved so far are lost."
        ))
        .action("Abort", ButtonVariant::Primary, move |_, cx| {
            this.update(cx, |this, cx| {
                // Not once another repository is on show: the operation asked about is this one's.
                if this.repository.id(cx) == &id {
                    this.abort(kind, cx);
                }
            });
        })
        .open(window, cx);
    }
}

impl Render for OperationBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.is_shown(cx) {
            return div().into_any_element();
        }
        let palette = *cx.palette();
        let operation = self.operation(cx);
        let files = self.files(cx);
        let first = self.first(cx);
        let summary = self.summary(cx);
        let pending = self.is_pending(cx);
        let title: SharedString = operation.as_ref().map_or_else(
            || "Conflicts".into(),
            |operation| operation_title(operation).into(),
        );
        let progress = operation.as_ref().and_then(operation_progress);

        let actions = operation.map(|operation| {
            let name = operation_name(&operation);
            let kind = operation.kind();
            let repository = self.repository.read(cx);
            let abort_state = repository.operation_state(ABORT);
            let proceed_state = repository.operation_state(CONTINUE);
            let label = if proceed_state.running {
                "Continuing…"
            } else {
                continue_label(kind)
            };
            let dismiss_abort = self.repository.entity().clone();
            let dismiss_proceed = dismiss_abort.clone();
            h_flex()
                .gap_1p5()
                .child(
                    FailurePopover::new(
                        "abort-failure",
                        format!("Abort {name}"),
                        abort_state.error,
                        action_button("abort")
                            .label("Abort")
                            .disabled(pending)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.confirm_abort(operation.clone(), window, cx)
                            })),
                    )
                    .on_dismiss(move |_, cx| {
                        dismiss_abort.update(cx, |repository, cx| repository.dismiss(ABORT, cx))
                    }),
                )
                .child(
                    FailurePopover::new(
                        "continue-failure",
                        format!("Continue {name}"),
                        proceed_state.error,
                        action_button("continue")
                            .primary()
                            .label(label)
                            .disabled(pending || !files.is_empty())
                            .on_click(cx.listener(move |this, _, _, cx| this.go_on(kind, cx))),
                    )
                    .on_dismiss(move |_, cx| {
                        dismiss_proceed
                            .update(cx, |repository, cx| repository.dismiss(CONTINUE, cx))
                    }),
                )
        });

        h_flex()
            .id("operation-bar")
            .role(Role::Status)
            .flex_none()
            .min_w_0()
            .gap_2()
            .px_3()
            .py_1p5()
            .border_b_1()
            .border_color(palette.border)
            .bg(palette.panel)
            .text_size(rems(0.8))
            .child(
                Icon::new(IconName::GitMergeConflict)
                    .size_4()
                    .flex_none()
                    .text_color(if files.is_empty() {
                        palette.mint
                    } else {
                        palette.coral
                    }),
            )
            .child(
                h_flex()
                    .min_w_0()
                    .items_baseline()
                    .gap_2()
                    .child(
                        div()
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette.text)
                            .child(title),
                    )
                    .when_some(progress, |this, progress| {
                        this.child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(palette.faint)
                                .child(progress),
                        )
                    })
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(palette.muted)
                            .child(summary),
                    ),
            )
            .child(
                h_flex()
                    .ml_auto()
                    .flex_none()
                    .gap_1p5()
                    .when_some(first, |this, file| {
                        this.child(
                            action_button("resolve-conflicts")
                                .ghost()
                                .label("Resolve conflicts")
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    cx.emit(OperationBarEvent::Resolve(file.clone()))
                                })),
                        )
                    })
                    .children(actions),
            )
            .into_any_element()
    }
}

/// The conflicted files in `repository`'s status, once it's loaded.
fn conflicted(repository: &Repository) -> Option<ConflictedFiles> {
    repository.status().data().map(select_conflicted)
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use futures::FutureExt as _;
    use gitto_git::features::operation::schema::Steps;
    use gitto_git::features::status::schema::{StatusReply, Uncommitted};
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};

    use super::*;
    use crate::components::repository::test_support::{
        self, conflicted, fake_git, repository_id, set_operation, set_status, setup, status,
        with_conflicts,
    };
    use gitto_app_data::data::DataError;
    use gitto_app_data::data::git::{GitTask, RepositoryGit};

    fn merge() -> Operation {
        Operation::Merge {
            merging: "feature".into(),
            into: Some("main".into()),
        }
    }

    fn rebase(step: u64, total: u64) -> Operation {
        Operation::Rebase {
            branch: Some("feature".into()),
            onto: "main".into(),
            steps: Some(Steps { step, total }),
        }
    }

    /// A repository's git that records what was continued and aborted, and what continuing leaves.
    #[derive(Default)]
    struct Calls {
        continued: RefCell<Vec<OperationKind>>,
        aborted: RefCell<Vec<OperationKind>>,
        /// The repository as continuing leaves it: the operation, and the status.
        after_continue: RefCell<Option<(Option<Operation>, Uncommitted)>>,
        /// The repository as it is, once it's been continued.
        now: RefCell<Option<(Option<Operation>, Uncommitted)>>,
    }

    impl RepositoryGit for Calls {
        fn continue_operation(&self, kind: OperationKind) -> GitTask<()> {
            self.continued.borrow_mut().push(kind);
            *self.now.borrow_mut() = self.after_continue.borrow_mut().take();
            async { Ok(()) }.boxed()
        }

        fn abort_operation(&self, kind: OperationKind) -> GitTask<()> {
            self.aborted.borrow_mut().push(kind);
            async { Ok(()) }.boxed()
        }

        // As the repository reloads after it.
        fn operation(&self) -> GitTask<Option<Operation>> {
            let now = self.now.borrow().clone();
            async move {
                now.map(|(operation, _)| operation)
                    .ok_or(DataError::msg("Unchanged."))
            }
            .boxed()
        }

        fn status(&self, _: Option<String>) -> GitTask<StatusReply> {
            let now = self.now.borrow().clone();
            async move {
                now.map(|(_, status)| StatusReply::Status(Box::new(status)))
                    .ok_or(DataError::msg("Unchanged."))
            }
            .boxed()
        }
    }

    struct Bar {
        _dir: tempfile::TempDir,
        window: AnyWindowHandle,
        bar: Entity<OperationBar>,
        resolved: Rc<RefCell<Vec<ChangedFile>>>,
        saved: Rc<Cell<bool>>,
        calls: Rc<Calls>,
    }

    fn render(cx: &mut TestAppContext, operation: Option<Operation>, status: Uncommitted) -> Bar {
        let dir = setup(cx);
        let id = repository_id();
        set_operation(&id, operation, cx);
        set_status(&id, status, cx);
        let calls = Rc::new(Calls::default());
        fake_git(&id, calls.clone(), cx);
        let saved = Rc::new(Cell::new(true));
        let resolved = Rc::new(RefCell::new(Vec::new()));
        let shown = id.clone();
        let (window, bar) = crate::test_support::open(cx, {
            let saved = Rc::clone(&saved);
            let resolved = Rc::clone(&resolved);
            move |window, cx| {
                let bar = cx.new(|cx| {
                    let mut bar = OperationBar::new(shown, window, cx);
                    bar.set_before_change(Rc::new(move |_| Task::ready(saved.get())));
                    bar
                });
                cx.subscribe(&bar, move |_, _, OperationBarEvent::Resolve(file), _| {
                    resolved.borrow_mut().push(file.clone())
                })
                .detach();
                Holder { bar }
            }
        });
        let bar = bar.read_with(cx, |holder, _| holder.bar.clone());
        Bar {
            _dir: dir,
            window,
            bar,
            resolved,
            saved,
            calls,
        }
    }

    struct Holder {
        bar: Entity<OperationBar>,
    }

    impl Render for Holder {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(self.bar.clone())
        }
    }

    fn click(bar: &Bar, id: &'static str, cx: &mut TestAppContext) {
        cx.update_window(bar.window, |_, window, cx| window.click(id, cx))
            .unwrap();
        cx.run_until_parked();
    }

    fn has_dialog(bar: &Bar, cx: &mut TestAppContext) -> bool {
        test_support::has_dialog(bar.window, cx)
    }

    /// Clicks the dialog's button labelled `label`.
    fn click_label(bar: &Bar, label: &str, cx: &mut TestAppContext) {
        test_support::click_label(bar.window, label, cx);
    }

    /// Clicks the last of what's labelled `label`: the dialog's own Abort, under the bar's.
    fn click_last_label(bar: &Bar, label: &str, cx: &mut TestAppContext) {
        test_support::click_last_label(bar.window, label, cx);
    }

    #[gpui_kit::test]
    fn isnt_there_without_an_operation_or_conflicts(cx: &mut TestAppContext) {
        let bar = render(cx, None, status());
        assert!(!bar.bar.read_with(cx, |bar, cx| bar.is_shown(cx)));
        let found = cx
            .update_window(bar.window, |_, window, _| {
                window.try_find("operation-bar").is_some()
            })
            .unwrap();
        assert!(!found);
    }

    #[gpui_kit::test]
    fn says_whats_under_way_and_only_continues_once_nothings_conflicted(cx: &mut TestAppContext) {
        let bar = render(
            cx,
            Some(merge()),
            with_conflicts(status(), &["a.txt", "b.txt"], &["b.txt"]),
        );
        let operation = bar.bar.read_with(cx, |bar, cx| bar.operation(cx)).unwrap();
        assert_eq!(operation_title(&operation), "Merging feature into main");
        assert_eq!(
            bar.bar.read_with(cx, |bar, cx| bar.summary(cx)),
            "2 files have conflicts, 1 ready to mark resolved"
        );
        assert!(!bar.bar.read_with(cx, |bar, cx| bar.may_continue(cx)));
        click(&bar, "continue", cx);
        assert!(bar.calls.continued.borrow().is_empty());

        // The one with markers left first.
        click(&bar, "resolve-conflicts", cx);
        assert_eq!(*bar.resolved.borrow(), [conflicted("a.txt")]);
    }

    #[gpui_kit::test]
    fn continues_once_every_conflict_is_resolved(cx: &mut TestAppContext) {
        let bar = render(cx, Some(rebase(3, 7)), status());
        let operation = bar.bar.read_with(cx, |bar, cx| bar.operation(cx)).unwrap();
        assert_eq!(operation_title(&operation), "Rebasing feature onto main");
        assert_eq!(operation_progress(&operation).as_deref(), Some("3/7"));
        assert_eq!(
            bar.bar.read_with(cx, |bar, cx| bar.summary(cx)),
            "All conflicts resolved"
        );
        click(&bar, "continue", cx);
        assert_eq!(*bar.calls.continued.borrow(), [OperationKind::Rebase]);
    }

    #[gpui_kit::test]
    fn opens_the_first_conflicted_file_once_continuing_stops_at_the_next_commits_conflicts(
        cx: &mut TestAppContext,
    ) {
        let bar = render(cx, Some(rebase(1, 2)), status());
        *bar.calls.after_continue.borrow_mut() = Some((
            Some(rebase(2, 2)),
            with_conflicts(status(), &["a.txt", "b.txt"], &["a.txt"]),
        ));

        click(&bar, "continue", cx);
        // The one with markers left first.
        assert_eq!(*bar.resolved.borrow(), [conflicted("b.txt")]);
        let operation = bar.bar.read_with(cx, |bar, cx| bar.operation(cx)).unwrap();
        assert_eq!(operation_progress(&operation).as_deref(), Some("2/2"));
        assert_eq!(
            bar.bar.read_with(cx, |bar, cx| bar.summary(cx)),
            "2 files have conflicts, 1 ready to mark resolved"
        );
    }

    #[gpui_kit::test]
    fn opens_nothing_once_continuing_finishes_the_operation(cx: &mut TestAppContext) {
        let bar = render(cx, Some(rebase(2, 2)), status());
        *bar.calls.after_continue.borrow_mut() = Some((None, status()));

        click(&bar, "continue", cx);
        assert!(!bar.bar.read_with(cx, |bar, cx| bar.is_shown(cx)));
        assert!(bar.resolved.borrow().is_empty());
    }

    #[gpui_kit::test]
    fn asks_before_aborting_which_throws_away_whats_resolved(cx: &mut TestAppContext) {
        let bar = render(cx, Some(merge()), with_conflicts(status(), &["a.txt"], &[]));

        click(&bar, "abort", cx);
        assert!(has_dialog(&bar, cx));
        click_label(&bar, "Cancel", cx);
        assert!(!has_dialog(&bar, cx));
        assert!(bar.calls.aborted.borrow().is_empty());

        click(&bar, "abort", cx);
        assert!(has_dialog(&bar, cx));
        click_last_label(&bar, "Abort", cx);
        assert_eq!(*bar.calls.aborted.borrow(), [OperationKind::Merge]);
    }

    #[gpui_kit::test]
    fn doesnt_abort_another_repositorys_operation_than_the_one_it_asked_about(
        cx: &mut TestAppContext,
    ) {
        let bar = render(cx, Some(merge()), with_conflicts(status(), &["a.txt"], &[]));
        click(&bar, "abort", cx);
        assert!(has_dialog(&bar, cx));

        // Another repository, merging too, comes on show under the dialog.
        let other = repository_id();
        set_operation(&other, Some(merge()), cx);
        set_status(&other, with_conflicts(status(), &["b.txt"], &[]), cx);
        bar.bar
            .update(cx, |bar, cx| bar.set_repository(other.clone(), cx));
        cx.run_until_parked();
        click_last_label(&bar, "Abort", cx);
        assert!(bar.calls.aborted.borrow().is_empty());
    }

    #[gpui_kit::test]
    fn continues_once_a_files_edits_are_saved_and_not_while_they_cant_be(cx: &mut TestAppContext) {
        let bar = render(cx, Some(rebase(3, 7)), status());
        bar.saved.set(false);
        click(&bar, "continue", cx);
        assert!(bar.calls.continued.borrow().is_empty());
        bar.saved.set(true);
        click(&bar, "continue", cx);
        assert_eq!(*bar.calls.continued.borrow(), [OperationKind::Rebase]);
    }

    #[gpui_kit::test]
    fn doesnt_abort_while_a_files_edits_cant_be_saved(cx: &mut TestAppContext) {
        let bar = render(cx, Some(merge()), with_conflicts(status(), &["a.txt"], &[]));
        bar.saved.set(false);
        bar.bar
            .update(cx, |bar, cx| bar.abort(OperationKind::Merge, cx));
        cx.run_until_parked();
        assert!(bar.calls.aborted.borrow().is_empty());
    }

    #[gpui_kit::test]
    fn shows_the_conflicts_a_popped_stash_left_without_an_operation_to_continue(
        cx: &mut TestAppContext,
    ) {
        let bar = render(cx, None, with_conflicts(status(), &["a.txt"], &[]));
        assert!(bar.bar.read_with(cx, |bar, cx| bar.is_shown(cx)));
        assert_eq!(
            bar.bar.read_with(cx, |bar, cx| bar.summary(cx)),
            "1 file has conflicts. Resolve them, then commit."
        );
        let abort = cx
            .update_window(bar.window, |_, window, _| {
                window.try_find("abort").is_some()
            })
            .unwrap();
        assert!(!abort);
    }

    #[test]
    fn labels_continuing_as_committing_a_merge() {
        assert_eq!(continue_label(OperationKind::Merge), "Commit merge");
        assert_eq!(continue_label(OperationKind::Rebase), "Continue");
    }
}
