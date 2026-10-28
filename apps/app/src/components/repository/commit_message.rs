//! The commit message editor that committing (`details::commit_form`) and rewording a commit
//! (`history::reword_dialog`) share: the summary and the description of a message, and what
//! they say about a commit that's pushed already.

use gitto_ui::git::message::CommitMessage;
use gpui_kit::component::input::{Enter, InputEvent, InputState, TextareaState};
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, EventEmitter, Focusable as _, Subscription, Window};

use crate::components::ui::FormField;
use crate::data::Query;

/// What the summary's placeholder says when no message is loading.
const SUMMARY_PLACEHOLDER: &str = "Summary of your changes";

/// What the editor tells its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitMessageEvent {
    /// The message was edited, by typing: it's now this.
    Edited(CommitMessage),
    /// Enter in the summary, or Cmd or Ctrl+Enter in either field, as a form's submit.
    Submit,
}

/// A commit message's summary and description, in two fields (TS's `CommitMessageFields`), for
/// an owner that commits with it. An entity: it tells its owner when the message is edited, or
/// submitted ([`CommitMessageEvent`]), and the owner puts a message in the fields
/// ([`set_message`](Self::set_message)), e.g. a commit's once it's loaded, and renders them
/// ([`fields`](Self::fields)).
pub struct CommitMessageEditor {
    summary: Entity<InputState>,
    description: Entity<TextareaState>,
    /// The message the fields were last given, or typed in them: what's typed reaches the owner
    /// after the field has it, so a message put in the fields meanwhile would undo it.
    shown: CommitMessage,
    /// Whether the fields were typed in since [`set_message`](Self::set_message) last said
    /// they're fresh.
    edited: bool,
    /// The summary's placeholder, as last set.
    placeholder: &'static str,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<CommitMessageEvent> for CommitMessageEditor {}

impl CommitMessageEditor {
    /// The fields, with a description of `rows` lines.
    pub fn new(rows: usize, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let summary = cx.new(|cx| InputState::new(window, cx).placeholder(SUMMARY_PLACEHOLDER));
        let description = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(rows)
                .placeholder("Add more context…")
        });
        let subscriptions = vec![
            cx.subscribe(&summary, |this, _, event, cx| match event {
                InputEvent::Change => this.typed(cx),
                InputEvent::PressEnter { .. } => cx.emit(CommitMessageEvent::Submit),
                _ => {}
            }),
            cx.subscribe(&description, |this, _, event, cx| {
                if let InputEvent::Change = event {
                    this.typed(cx)
                }
            }),
        ];
        CommitMessageEditor {
            summary,
            description,
            shown: CommitMessage::default(),
            edited: false,
            placeholder: SUMMARY_PLACEHOLDER,
            _subscriptions: subscriptions,
        }
    }

    pub fn summary(&self) -> &Entity<InputState> {
        &self.summary
    }

    pub fn description(&self) -> &Entity<TextareaState> {
        &self.description
    }

    /// The message as the fields have it.
    pub fn message(&self) -> &CommitMessage {
        &self.shown
    }

    /// The summary's placeholder: what to type there, or what's loading.
    pub fn placeholder(&self) -> &'static str {
        self.placeholder
    }

    /// Whether the fields were typed in since they were last given a fresh message.
    pub fn is_edited(&self) -> bool {
        self.edited
    }

    /// Puts `message` in the fields, where they don't have it already; `fresh`, it's one they
    /// haven't been typed in since (see [`is_edited`](Self::is_edited)).
    pub fn set_message(
        &mut self,
        message: &CommitMessage,
        fresh: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if fresh {
            self.edited = false;
        }
        if self.shown == *message {
            return;
        }
        self.shown = message.clone();
        let (summary, description) = (message.summary.clone(), message.description.clone());
        self.summary.update(cx, |input, cx| {
            if input.value().as_ref() != summary {
                input.set_value(summary, window, cx);
            }
        });
        self.description.update(cx, |input, cx| {
            if input.value().as_ref() != description {
                input.set_value(description, window, cx);
            }
        });
        cx.notify();
    }

    /// Says in the summary's placeholder what's loading (`loading`), e.g. the commit's message;
    /// or, with nothing loading, what to type there.
    pub fn set_loading(
        &mut self,
        loading: Option<&'static str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let placeholder = loading.unwrap_or(SUMMARY_PLACEHOLDER);
        if self.placeholder != placeholder {
            self.placeholder = placeholder;
            self.summary.update(cx, |input, cx| {
                input.set_placeholder(placeholder, window, cx)
            });
        }
    }

    /// Gives the summary the focus.
    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.summary.focus_handle(cx).focus(window, cx);
    }

    /// The fields were typed in: the owner's told, if that changed the message.
    fn typed(&mut self, cx: &mut Context<Self>) {
        let message = CommitMessage::new(
            self.summary.read(cx).value().to_string(),
            self.description.read(cx).value().to_string(),
        );
        if message == self.shown {
            return;
        }
        self.shown = message.clone();
        self.edited = true;
        cx.emit(CommitMessageEvent::Edited(message));
        cx.notify();
    }

    /// The fields of `editor`, for its owner to render: read-only while `locked` (which keeps
    /// their focus, e.g. to type the next message while a commit's made). Cmd or Ctrl+Enter
    /// submits from either, rather than starting a line in the description.
    pub fn fields(editor: &Entity<Self>, locked: bool, cx: &App) -> impl IntoElement + use<> {
        let this = editor.read(cx);
        let submit = editor.downgrade();
        v_flex()
            .capture_action(move |action: &Enter, _, cx| {
                if action.secondary {
                    cx.stop_propagation();
                    let _ = submit.update(cx, |_, cx| cx.emit(CommitMessageEvent::Submit));
                }
            })
            .child(FormField::input("Commit message", &this.summary).readonly(locked))
            .child(
                FormField::textarea("Description", &this.description)
                    .hint("optional")
                    .readonly(locked),
            )
    }
}

/// What to know before rewriting `commit` (e.g. "the last commit") by `rewriting` it (e.g.
/// "amending it"), from where it's pushed to (`pushed_to`): it's already on a remote branch, or
/// whether it is couldn't be checked; rewriting it then means force-pushing. Nothing while that's
/// being checked.
pub fn push_warning(
    pushed_to: &Query<Option<String>>,
    commit: &str,
    rewriting: &str,
) -> Option<String> {
    if pushed_to.is_error() {
        return Some(format!(
            "Couldn't check whether {commit} has been pushed; if it has, {rewriting} means \
             force-pushing."
        ));
    }
    let branch = pushed_to
        .data()
        .flatten()
        .filter(|_| pushed_to.is_success())?;
    Some(format!(
        "{} is already on {branch}; {rewriting} means force-pushing.",
        capitalized(commit)
    ))
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
