//! Editing the last commit's message. Ported from
//! `components/repository/history/reword-dialog.tsx`.

use gitto_git::input::RepositoryId;
use gitto_ui_logic::git::message::{CommitMessage, edit_message, split_message};
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, Rems, Subscription,
    Window, div,
};

use super::columns::SMALL_TEXT;
use crate::components::repository::commit_message::{
    CommitMessageEditor, CommitMessageEvent, push_warning,
};
use gitto_app_data::data::commit::{Reword, reword_commit_mutation};
use gitto_app_data::data::keyed::Hold;
use gitto_app_data::data::{Changed, DataError, Loaded, Mutation, Part, Repositories, Repository};
use gitto_app_kit::theme::{PaletteExt as _, base_px};
use gitto_app_kit::ui::{DialogId, Icon, IconName};

/// How wide the dialog is (`max-w-[440px]`), in rems of the 15px base.
const WIDTH: Rems = base_px(440.);

/// Rewording a commit: the repository and the commit's SHA, and the message to give it.
pub type RewordMutation = Mutation<(RepositoryId, Reword), String>;

/// What the dialog tells its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RewordEvent {
    /// A commit was reworded, by its SHA before and after, e.g. to keep it selected.
    Reworded { from: String, to: String },
}

/// Edits the message of the last commit of the checked-out branch, and rewrites the commit with
/// it; open while it's [`open`](Self::open) for a commit. What's staged is left out of the commit,
/// and stays staged. Like amending in the commit form, it says first when the commit is pushed
/// already, as the branch then needs force-pushing. It stays open if that fails, saying why, with
/// the edits kept. An entity, which the dialog shows: it re-renders as the message loads.
pub struct RewordDialog {
    repository_id: RepositoryId,
    repository: Entity<Repository>,
    /// The commit whose message is being edited, while it's open.
    sha: Option<String>,
    /// Holds the commit's message and where it was pushed, while it's open.
    holds: Option<(Hold, Hold)>,
    /// The dialog, while it's open.
    dialog: Option<DialogId>,
    reword: Entity<RewordMutation>,
    editor: Entity<CommitMessageEditor>,
    /// The message the fields were filled with once it loaded: until then they're locked, so no
    /// edits stand in for it.
    loaded: Option<CommitMessage>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
    _loading: Option<Subscription>,
}

impl EventEmitter<RewordEvent> for RewordDialog {}

impl Focusable for RewordDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl RewordDialog {
    pub fn new(repository_id: RepositoryId, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let reword = reword_commit_mutation(cx);
        let editor = cx.new(|cx| CommitMessageEditor::new(5, window, cx));
        let subscriptions = vec![
            cx.observe(&reword, |_, _, cx| cx.notify()),
            cx.subscribe_in(&editor, window, |this, _, event, window, cx| match event {
                CommitMessageEvent::Edited(_) => cx.notify(),
                CommitMessageEvent::Submit => this.submit(window, cx),
            }),
        ];
        RewordDialog {
            repository: Repositories::open(&repository_id, cx),
            repository_id,
            sha: None,
            holds: None,
            dialog: None,
            reword,
            editor,
            loaded: None,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
            _loading: None,
        }
    }

    /// Of another repository: a commit of one isn't one of another, so it closes.
    pub fn set_repository(
        &mut self,
        repository_id: RepositoryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.repository_id != repository_id {
            self.close(window, cx);
            self.repository = Repositories::open(&repository_id, cx);
            self.repository_id = repository_id;
        }
    }

    /// The commit it's open for, if it's open.
    pub fn sha(&self) -> Option<&str> {
        self.sha.as_deref()
    }

    /// Opens it for the commit `sha`. Each time it opens, it starts from the commit's message,
    /// with no failure from before.
    pub fn open(&mut self, sha: String, window: &mut Window, cx: &mut Context<Self>) {
        let was_open = self.sha.is_some();
        self.reword.update(cx, |reword, cx| reword.reset(cx));
        self.loaded = None;
        self.holds = Some(self.repository.update(cx, |repository, cx| {
            (
                repository.hold_message(&sha, cx),
                repository.hold_pushed_to(&sha, cx),
            )
        }));
        self._loading = Some(cx.subscribe_in(
            &self.repository,
            window,
            |this, _, Changed(part), window, cx| {
                if matches!(part, Part::Commits | Part::PushedTo) {
                    this.fill(window, cx);
                    cx.notify();
                }
            },
        ));
        self.sha = Some(sha);
        self.editor.update(cx, |editor, cx| {
            editor.set_message(&CommitMessage::default(), true, window, cx)
        });
        self.fill(window, cx);
        if !was_open {
            let this = cx.entity();
            self.dialog = Some(DialogId::open(window, cx, move |_, dialog, window, _| {
                let cancel = this.clone();
                dialog
                    .w(WIDTH.to_pixels(window.rem_size()))
                    .close_button(false)
                    // Enter in the summary saves (see `submit`), which closes it once it's done:
                    // not at once, as it stays open if that fails.
                    .on_ok(|_, _, _| false)
                    .on_cancel(move |_, window, cx| {
                        cancel.update(cx, |this, cx| this.closed(window, cx));
                        true
                    })
                    .child(this.clone())
            }));
        }
        self.editor
            .update(cx, |editor, cx| editor.focus(window, cx));
        cx.notify();
    }

    /// Closes it, if it's open.
    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(dialog) = self.dialog.take() {
            dialog.close(window, cx);
            self.closed(window, cx);
        }
    }

    /// It's closed: it no longer loads the message.
    fn closed(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.sha = None;
        self.dialog = None;
        self.holds = None;
        self._loading = None;
        cx.notify();
    }

    /// Puts the commit's message in the fields once it's loaded, unless they've been edited.
    fn fill(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let loading = self.loaded_message(cx).is_some_and(Loaded::is_pending);
        self.editor.update(cx, |editor, cx| {
            editor.set_loading(loading.then_some("Loading the message…"), window, cx)
        });
        if self.loaded.is_some() {
            return;
        }
        let Some(original) = self.original(cx) else {
            return;
        };
        let message = split_message(&original);
        self.editor.update(cx, |editor, cx| {
            if !editor.is_edited() {
                editor.set_message(&message, false, window, cx);
            }
        });
        self.loaded = Some(message);
        cx.notify();
    }

    /// The message as the fields have it.
    pub fn message(&self, cx: &App) -> CommitMessage {
        self.editor.read(cx).message().clone()
    }

    /// The commit's message, as far as it's loaded, while it's open.
    fn loaded_message<'a>(&self, cx: &'a App) -> Option<&'a Loaded<String>> {
        self.repository.read(cx).message(self.sha.as_ref()?)
    }

    /// Which remote branch has the commit, as far as that's loaded, while it's open.
    fn pushed_to<'a>(&self, cx: &'a App) -> Option<&'a Loaded<Option<String>>> {
        self.repository.read(cx).pushed_to(self.sha.as_ref()?)
    }

    /// The commit's message as written, once it's loaded.
    fn original(&self, cx: &App) -> Option<String> {
        self.loaded_message(cx)
            .filter(|message| message.is_success())
            .and_then(Loaded::data)
            .cloned()
    }

    /// The message to save: what wasn't edited of it, exactly as written, not as the fields
    /// split it.
    fn edited_message(&self, cx: &App) -> Option<String> {
        self.original(cx)
            .map(|original| edit_message(&original, &self.message(cx)))
    }

    /// Also locked if the message failed to load (the error says so), so no edits stand in for
    /// it.
    fn is_locked(&self, cx: &App) -> bool {
        self.loaded.is_none() || self.reword.read(cx).is_pending()
    }

    /// Whether there's an edited message to save, once it's known whether the commit was pushed,
    /// so its warning is seen before rewording.
    pub fn is_savable(&self, cx: &App) -> bool {
        let edited = self.edited_message(cx);
        !self.message(cx).summary.trim().is_empty()
            && edited.is_some()
            && edited != self.original(cx)
            && !self.pushed_to(cx).is_none_or(Loaded::is_pending)
            && !self.reword.read(cx).is_pending()
    }

    /// Rewords the commit with the edited message; closes once it's done, unless it's open for
    /// another commit by then, and tells its owner the commit's new SHA.
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(from), Some(message)) = (self.sha.clone(), self.edited_message(cx)) else {
            return;
        };
        if !self.is_savable(cx) {
            return;
        }
        let input = match Reword::parse(from.clone(), message) {
            Ok(reword) => (self.repository_id.clone(), reword),
            Err(error) => {
                let error = self.repository.read(cx).git().invalid(error);
                self.reword
                    .update(cx, |reword, cx| reword.refuse(error, cx));
                return;
            }
        };
        let done = self
            .reword
            .update(cx, |reword, cx| reword.mutate(input, cx));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(to)) = done.await else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                cx.emit(RewordEvent::Reworded {
                    from: from.clone(),
                    to,
                });
                if this.sha.as_deref() == Some(from.as_str()) {
                    this.close(window, cx);
                }
            });
        })
        .detach();
    }

    /// Why it can't be reworded, or its message couldn't be loaded; not whether it's been
    /// pushed: that only decides the warning.
    fn error(&self, cx: &App) -> Option<DataError> {
        self.reword
            .read(cx)
            .error()
            .cloned()
            .or_else(|| self.loaded_message(cx).and_then(Loaded::error).cloned())
    }

    /// What it says under the fields: that the commit is pushed already, or that couldn't be
    /// checked, so the branch would need force-pushing; and why rewording failed.
    pub fn notes(&self, cx: &App) -> Vec<(Note, String)> {
        let mut notes = Vec::new();
        let warning = push_warning(self.pushed_to(cx), "the commit", "editing its message");
        if let Some(warning) = warning {
            notes.push((Note::Warning, warning));
        }
        if let Some(error) = self.error(cx) {
            notes.push((Note::Error, error.to_string()));
        }
        notes
    }
}

/// What a note under the dialog's fields is: a warning, or why something failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Note {
    Warning,
    Error,
}

impl Render for RewordDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = *cx.palette();
        let locked = self.is_locked(cx);
        let notes = self.notes(cx);
        let savable = self.is_savable(cx);
        v_flex()
            .id("reword-dialog")
            .track_focus(&self.focus)
            .child(
                div()
                    .text_size(base_px(12.5))
                    .font_weight(FontWeight(680.))
                    .text_color(palette.text)
                    .child("Edit commit message"),
            )
            .child(
                div()
                    .mt_1()
                    .mb_2p5()
                    .text_size(SMALL_TEXT)
                    .text_color(palette.muted)
                    .child(
                        "Rewrites the last commit with the new message. What's staged isn't \
                         added to it.",
                    ),
            )
            // Cmd or Ctrl+Enter saves from either field, as Enter does in the summary.
            .child(CommitMessageEditor::fields(&self.editor, locked, cx))
            .children(notes.into_iter().map(|(note, text)| {
                div()
                    .mb_2p5()
                    .text_size(SMALL_TEXT)
                    .text_color(match note {
                        Note::Warning => palette.amber,
                        Note::Error => palette.coral,
                    })
                    .child(text)
            }))
            .child(
                h_flex()
                    .mt_1()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("cancel")
                            .ghost()
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
                    )
                    .child(
                        Button::new("save-message")
                            .primary()
                            .icon(Icon::new(IconName::PenLine))
                            .label("Save message")
                            .disabled(!savable)
                            .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                    ),
            )
    }
}
