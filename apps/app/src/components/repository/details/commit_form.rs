//! Committing what's staged, or amending the last commit. Ported from
//! `components/repository/details/commit-form.tsx`.

use gitto_git::input::RepositoryId;
use gitto_ui_logic::git::message::{CommitMessage, edit_message, join_message, split_message};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, FontWeight, SharedString, Subscription, Window, div, rems};

use crate::components::repository::commit_message::{
    CommitMessageEditor, CommitMessageEvent, push_warning,
};
use gitto_app_data::data::commit::{NewCommit, create_commit_mutation};
use gitto_app_data::data::keyed::Hold;
use gitto_app_data::data::{
    DataError, Loaded, Mutation, Part, Repositories, Repository, observe_parts,
};
use gitto_app_kit::theme::{PaletteExt as _, base_px};
use gitto_app_kit::ui::{Icon, IconName};

/// The shortcut that commits, as the button shows it.
const SHORTCUT: &str = if cfg!(target_os = "macos") {
    "⌘ ↵"
} else {
    "Ctrl ↵"
};

/// HEAD's commit, told apart from the same commit in another repository, e.g. another worktree.
#[derive(Debug, Clone, PartialEq)]
struct Head {
    repository: RepositoryId,
    sha: String,
}

/// Commits what's staged in the repository, or amends the last commit with it. The new commit's
/// message and the amended one are kept apart, so switching between them loses neither. Its owner
/// tells it how many files are staged and which commit HEAD is ([`set_head`](Self::set_head)).
pub struct CommitForm {
    repository_id: RepositoryId,
    staged_count: usize,
    /// The commit to amend, HEAD's; `None` before the first commit.
    last_commit: Option<String>,
    /// The repository `repository_id`, which loads the last commit's message and where it was
    /// pushed.
    repository: Entity<Repository>,
    commit: Entity<Mutation<(RepositoryId, NewCommit)>>,
    /// The commit being amended (as [`head`](Self::head)). Amending stops once HEAD moves to
    /// another commit: after the amend, or when one is made or checked out elsewhere, so a stale
    /// message is never amended onto it.
    amending: Option<Head>,
    draft: CommitMessage,
    /// Edits to the amended message, kept for the commit they were made to while amending is
    /// toggled.
    edits: Option<(Head, CommitMessage)>,
    /// Holds the last commit's message and where it was pushed, loaded while amending it.
    amend_holds: Option<(Hold, Hold)>,
    editor: Entity<CommitMessageEditor>,
    _repository_changes: Subscription,
    _subscriptions: Vec<Subscription>,
}

/// What the form shows of the repository.
const PARTS: &[Part] = &[Part::Commits, Part::PushedTo];

impl CommitForm {
    pub fn new(repository_id: RepositoryId, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let commit = create_commit_mutation(cx);
        let repository = Repositories::open(&repository_id, cx);
        let repository_changes = observe_parts(&repository, PARTS, cx);
        let editor = cx.new(|cx| CommitMessageEditor::new(3, window, cx));
        let subscriptions = vec![
            cx.observe(&commit, |_, _, cx| cx.notify()),
            // Whatever changes the message on show notifies it: amending turned on or off, the
            // last commit's message loaded, a commit made, HEAD moved.
            cx.observe_in(&cx.entity(), window, |this, _, window, cx| {
                this.show(window, cx)
            }),
            cx.subscribe(&editor, |this, _, event, cx| match event {
                CommitMessageEvent::Edited(message) => this.edit(message.clone(), cx),
                CommitMessageEvent::Submit => this.submit(cx),
            }),
        ];
        CommitForm {
            repository_id,
            staged_count: 0,
            last_commit: None,
            repository,
            commit,
            amending: None,
            draft: CommitMessage::default(),
            edits: None,
            amend_holds: None,
            editor,
            _repository_changes: repository_changes,
            _subscriptions: subscriptions,
        }
    }

    /// How many files are staged, and HEAD's commit (`None` before the first), in the repository
    /// `repository_id`. Once HEAD moves (or is another repository's, e.g. another worktree's at
    /// the same commit), amending stops, without loading the new commit's message.
    pub fn set_head(
        &mut self,
        repository_id: RepositoryId,
        staged_count: usize,
        last_commit: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let moved = self.repository_id != repository_id || self.last_commit != last_commit;
        if !moved && self.staged_count == staged_count {
            return;
        }
        self.staged_count = staged_count;
        if moved {
            if self.repository_id != repository_id {
                self.repository = Repositories::open(&repository_id, cx);
                self._repository_changes = observe_parts(&self.repository, PARTS, cx);
            }
            self.repository_id = repository_id;
            self.last_commit = last_commit;
            self.amending = None;
            self.amend_holds = None;
        }
        cx.notify();
    }

    fn head(&self) -> Option<Head> {
        Some(Head {
            repository: self.repository_id.clone(),
            sha: self.last_commit.clone()?,
        })
    }

    /// Whether `head` is HEAD.
    fn is_head(&self, head: &Head) -> bool {
        head.repository == self.repository_id && self.last_commit.as_ref() == Some(&head.sha)
    }

    /// Whether it's amending HEAD: checked against HEAD too, as amending is only reset once HEAD
    /// is seen to move.
    pub fn is_amending(&self) -> bool {
        self.amending
            .as_ref()
            .is_some_and(|head| self.is_head(head))
    }

    /// The last commit's message, as far as it's loaded.
    fn last_message<'a>(&self, cx: &'a App) -> Option<&'a Loaded<String>> {
        self.repository.read(cx).message(self.last_commit.as_ref()?)
    }

    /// Which remote branch has the last commit, as far as that's loaded.
    fn pushed_to<'a>(&self, cx: &'a App) -> Option<&'a Loaded<Option<String>>> {
        self.repository
            .read(cx)
            .pushed_to(self.last_commit.as_ref()?)
    }

    /// The last commit's message, once loaded, and if the last load didn't fail.
    fn original(&self, cx: &App) -> Option<String> {
        self.last_message(cx)
            .filter(|message| message.is_success())
            .and_then(Loaded::data)
            .cloned()
    }

    /// The last commit's message, split, once loaded.
    fn loaded(&self, cx: &App) -> Option<CommitMessage> {
        self.original(cx).map(|message| split_message(&message))
    }

    /// Whether the fields wait: for the last commit's message while amending (also if it failed
    /// to load, which the error says, so no edits stand in for it), or for the commit.
    fn is_locked(&self, cx: &App) -> bool {
        (self.is_amending() && self.loaded(cx).is_none()) || self.commit.read(cx).is_pending()
    }

    fn amend_edits(&self) -> Option<CommitMessage> {
        self.edits
            .as_ref()
            .filter(|(edited, _)| self.is_head(edited))
            .map(|(_, message)| message.clone())
    }

    /// The message on show: the amended one's, or the new commit's.
    pub fn message(&self, cx: &App) -> CommitMessage {
        if self.is_amending() {
            self.amend_edits()
                .or_else(|| self.loaded(cx))
                .unwrap_or_default()
        } else {
            self.draft.clone()
        }
    }

    /// The message on show was typed as `message`.
    fn edit(&mut self, message: CommitMessage, cx: &mut Context<Self>) {
        if message == self.message(cx) {
            return;
        }
        match self.head().filter(|_| self.is_amending()) {
            Some(head) => self.edits = Some((head, message)),
            None => self.draft = message,
        }
        cx.notify();
    }

    /// Turns amending on or off; either way, the last commit's failure is forgotten.
    pub fn toggle_amend(&mut self, checked: bool, cx: &mut Context<Self>) {
        self.amending = if checked { self.head() } else { None };
        self.amend_holds = self
            .last_commit
            .clone()
            .filter(|_| self.is_amending())
            .map(|sha| {
                self.repository.update(cx, |repository, cx| {
                    (
                        repository.hold_message(&sha, cx),
                        repository.hold_pushed_to(&sha, cx),
                    )
                })
            });
        self.commit.update(cx, |commit, cx| commit.reset(cx));
        cx.notify();
    }

    /// The message to amend with, once the last one has loaded: what wasn't edited of it exactly
    /// as written, not as the form split it into two fields.
    fn amended_message(&self, cx: &App) -> Option<String> {
        let original = self.original(cx)?;
        Some(edit_message(&original, &self.message(cx)))
    }

    /// Whether amending changes anything: with nothing staged and the same message, it would
    /// only rewrite the commit's date.
    fn amend_changes(&self, cx: &App) -> bool {
        self.staged_count > 0
            || self
                .amended_message(cx)
                .is_some_and(|amended| Some(amended) != self.original(cx))
    }

    pub fn can_commit(&self, cx: &App) -> bool {
        !self.message(cx).summary.trim().is_empty()
            // Once the push check is done too, so its warning is seen before amending.
            && if self.is_amending() {
                self.amend_changes(cx) && !self.pushed_to(cx).is_none_or(Loaded::is_pending)
            } else {
                self.staged_count > 0
            }
            && !self.commit.read(cx).is_pending()
    }

    /// Commits, or amends, if it can.
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        if !self.can_commit(cx) {
            return;
        }
        let amended = self.last_commit.clone().filter(|_| self.is_amending());
        let message = amended
            .as_ref()
            .and_then(|_| self.amended_message(cx))
            .unwrap_or_else(|| join_message(&self.message(cx)));
        let input = match NewCommit::parse(message, amended.clone()) {
            Ok(commit) => (self.repository_id.clone(), commit),
            Err(error) => {
                let error = self.repository.read(cx).git().invalid(error);
                self.commit
                    .update(cx, |commit, cx| commit.refuse(error, cx));
                return;
            }
        };
        let done = self
            .commit
            .update(cx, |commit, cx| commit.mutate(input, cx));
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(())) = done.await {
                let _ = this.update(cx, |this, cx| {
                    if amended.is_none() {
                        this.draft = CommitMessage::default();
                    } else {
                        this.amending = None;
                        this.edits = None;
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// The button's label, which says what it does.
    pub fn label(&self) -> String {
        let count = self.staged_count;
        let files = format!("{count} {}", if count == 1 { "file" } else { "files" });
        match (self.is_amending(), count > 0) {
            (true, true) => format!("Amend with {files}"),
            (true, false) => "Amend message".to_owned(),
            (false, true) => format!("Commit {files}"),
            (false, false) => "Nothing staged".to_owned(),
        }
    }

    /// Why the commit, or the amend's last message, failed, if either did. Not whether it's been
    /// pushed: that only decides the warning, and doesn't stop amending.
    pub fn error(&self, cx: &App) -> Option<DataError> {
        self.commit.read(cx).error().cloned().or_else(|| {
            self.last_message(cx)
                .and_then(Loaded::error)
                .filter(|_| self.is_amending())
                .cloned()
        })
    }

    /// What to know before amending: the last commit is already on a remote branch, or whether it
    /// is couldn't be checked; amending it then means force-pushing.
    pub fn push_warning(&self, cx: &App) -> Option<String> {
        if !self.is_amending() {
            return None;
        }
        push_warning(self.pushed_to(cx), "the last commit", "amending it")
    }

    /// Puts the message on show in the fields once it changed other than by typing (amending
    /// turned on or off, the last message loaded, a commit made, HEAD moved), and says in the
    /// summary's placeholder when the last commit's message is loading.
    fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let message = self.message(cx);
        let loading = self.is_amending() && self.last_message(cx).is_none_or(Loaded::is_pending);
        self.editor.update(cx, |editor, cx| {
            editor.set_message(&message, false, window, cx);
            editor.set_loading(loading.then_some("Loading the last commit…"), window, cx);
        });
    }
}

impl Render for CommitForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = *cx.palette();
        let amend = self.is_amending();
        let locked = self.is_locked(cx);
        let pending = self.commit.read(cx).is_pending();
        let warning = self.push_warning(cx);
        let error = self.error(cx);
        let can_commit = self.can_commit(cx);
        let label = self.label();

        v_flex()
            .id("commit-form")
            .flex_none()
            .p(base_px(14.))
            .child(CommitMessageEditor::fields(&self.editor, locked, cx))
            .child(
                div().mb_2p5().child(
                    Checkbox::new("amend")
                        .small()
                        .label("Amend last commit")
                        .text_xs()
                        .text_color(palette.muted)
                        .checked(amend)
                        .disabled(self.last_commit.is_none() || pending)
                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                            this.toggle_amend(*checked, cx)
                        })),
                ),
            )
            .when_some(warning, |this, warning| {
                this.child(
                    div()
                        .id("push-warning")
                        .mb_2p5()
                        .text_xs()
                        .line_height(rems(1.1))
                        .text_color(palette.amber)
                        .whitespace_normal()
                        .child(warning),
                )
            })
            .child(
                Button::new("commit")
                    .primary()
                    .w_full()
                    .icon(Icon::new(if amend {
                        IconName::PenLine
                    } else {
                        IconName::GitCommitHorizontal
                    }))
                    .label(label.clone())
                    .accessibility_label(SharedString::from(format!("{label} {SHORTCUT}")))
                    .font_weight(FontWeight::BOLD)
                    .disabled(!can_commit)
                    .on_click(cx.listener(|this, _, _, cx| this.submit(cx)))
                    // The shortcut, quietly, in the button's text color (TS's borderless `Kbd`).
                    .child(
                        div()
                            .ml_auto()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_xs()
                            .opacity(0.65)
                            .child(SHORTCUT),
                    ),
            )
            .when_some(error, |this, error| {
                this.child(
                    div()
                        .id("commit-error")
                        .mt_2p5()
                        .text_xs()
                        .text_color(palette.coral)
                        .whitespace_normal()
                        .child(error.to_string()),
                )
            })
    }
}

#[cfg(test)]
mod tests;
