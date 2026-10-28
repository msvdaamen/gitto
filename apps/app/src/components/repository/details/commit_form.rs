//! Committing what's staged, or amending the last commit. Ported from
//! `components/repository/details/commit-form.tsx`.

use gitto_git::input::RepositoryId;
use gitto_ui::git::message::{CommitMessage, edit_message, join_message, split_message};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{Context, Entity, FontWeight, SharedString, Subscription, Window, div, rems};

use super::details_data::DetailsData;
use crate::components::repository::commit_message::{
    CommitMessageEditor, CommitMessageEvent, push_warning,
};
use crate::components::ui::{Icon, IconName};
use crate::data::commit::NewCommit;
use crate::data::{Mutation, Query, QueryError};
use crate::theme::{PaletteExt as _, base_px};

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
    data: DetailsData,
    commit: Entity<Mutation<(RepositoryId, NewCommit)>>,
    /// The commit being amended (as [`head`](Self::head)). Amending stops once HEAD moves to
    /// another commit: after the amend, or when one is made or checked out elsewhere, so a stale
    /// message is never amended onto it.
    amending: Option<Head>,
    draft: CommitMessage,
    /// Edits to the amended message, kept for the commit they were made to while amending is
    /// toggled.
    edits: Option<(Head, CommitMessage)>,
    /// The last commit's message and where it was pushed, loaded while amending it.
    last_message: Query<String>,
    pushed_to: Query<Option<String>>,
    editor: Entity<CommitMessageEditor>,
    _subscriptions: Vec<Subscription>,
}

impl CommitForm {
    pub fn new(
        repository_id: RepositoryId,
        data: DetailsData,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let commit = data.create_commit(cx);
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
        let last_message = Query::new(
            data.commit_message(&repository_id, "", cx).enabled(false),
            cx,
        );
        let pushed_to = Query::new(data.pushed_to(&repository_id, "", cx).enabled(false), cx);
        CommitForm {
            repository_id,
            staged_count: 0,
            last_commit: None,
            data,
            commit,
            amending: None,
            draft: CommitMessage::default(),
            edits: None,
            last_message,
            pushed_to,
            editor,
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
            self.repository_id = repository_id;
            self.last_commit = last_commit;
            self.amending = None;
            let sha = self.last_commit.clone().unwrap_or_default();
            let message = self
                .data
                .commit_message(&self.repository_id, &sha, cx)
                .enabled(false);
            self.last_message.set_options(message, cx);
            let pushed_to = self
                .data
                .pushed_to(&self.repository_id, &sha, cx)
                .enabled(false);
            self.pushed_to.set_options(pushed_to, cx);
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

    /// The last commit's message, split, once loaded.
    fn loaded(&self) -> Option<CommitMessage> {
        if !self.last_message.is_success() {
            return None;
        }
        self.last_message
            .data()
            .map(|message| split_message(&message))
    }

    /// Whether the fields wait: for the last commit's message while amending (also if it failed
    /// to load, which the error says, so no edits stand in for it), or for the commit.
    fn is_locked(&self, cx: &gpui_kit::App) -> bool {
        (self.is_amending() && self.loaded().is_none()) || self.commit.read(cx).is_pending()
    }

    fn amend_edits(&self) -> Option<CommitMessage> {
        self.edits
            .as_ref()
            .filter(|(edited, _)| self.is_head(edited))
            .map(|(_, message)| message.clone())
    }

    /// The message on show: the amended one's, or the new commit's.
    pub fn message(&self) -> CommitMessage {
        if self.is_amending() {
            self.amend_edits()
                .or_else(|| self.loaded())
                .unwrap_or_default()
        } else {
            self.draft.clone()
        }
    }

    /// The message on show was typed as `message`.
    fn edit(&mut self, message: CommitMessage, cx: &mut Context<Self>) {
        if message == self.message() {
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
        let amend = self.is_amending();
        self.last_message.set_enabled(amend, cx);
        self.pushed_to.set_enabled(amend, cx);
        self.commit.update(cx, |commit, cx| commit.reset(cx));
        cx.notify();
    }

    /// The message to amend with, once the last one has loaded: what wasn't edited of it exactly
    /// as written, not as the form split it into two fields.
    fn amended_message(&self) -> Option<String> {
        self.loaded()?;
        let original = self.last_message.data()?;
        Some(edit_message(&original, &self.message()))
    }

    /// Whether amending changes anything: with nothing staged and the same message, it would
    /// only rewrite the commit's date.
    fn amend_changes(&self) -> bool {
        self.staged_count > 0
            || self
                .amended_message()
                .is_some_and(|amended| Some(amended) != self.last_message.data())
    }

    pub fn can_commit(&self, cx: &gpui_kit::App) -> bool {
        !self.message().summary.trim().is_empty()
            // Once the push check is done too, so its warning is seen before amending.
            && if self.is_amending() {
                self.amend_changes() && !self.pushed_to.is_pending()
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
            .and_then(|_| self.amended_message())
            .unwrap_or_else(|| join_message(&self.message()));
        let input = (
            self.repository_id.clone(),
            NewCommit {
                message,
                amend: amended.clone(),
            },
        );
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
    pub fn error(&self, cx: &gpui_kit::App) -> Option<QueryError> {
        self.commit
            .read(cx)
            .error()
            .cloned()
            .or_else(|| self.last_message.error().filter(|_| self.is_amending()))
    }

    /// What to know before amending: the last commit is already on a remote branch, or whether it
    /// is couldn't be checked; amending it then means force-pushing.
    pub fn push_warning(&self) -> Option<String> {
        if !self.is_amending() {
            return None;
        }
        push_warning(&self.pushed_to, "the last commit", "amending it")
    }

    /// Puts the message on show in the fields once it changed other than by typing (amending
    /// turned on or off, the last message loaded, a commit made, HEAD moved), and says in the
    /// summary's placeholder when the last commit's message is loading.
    fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let message = self.message();
        let loading = self.is_amending() && self.last_message.is_pending();
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
        let warning = self.push_warning();
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
                        .child(error.message()),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AnyWindowHandle, TestAppContext};

    use super::*;
    use crate::components::repository::details::test_fakes::{
        Fakes, open, other_repository_id, repository_id,
    };

    /// The form, with files staged and HEAD at a commit, in a window.
    struct Form {
        _dir: tempfile::TempDir,
        window: AnyWindowHandle,
        form: Entity<CommitForm>,
        fakes: Rc<Fakes>,
    }

    fn render_form(cx: &mut TestAppContext, staged: usize, last_commit: Option<&str>) -> Form {
        let fakes = Fakes::new();
        let data = fakes.data();
        let last_commit = last_commit.map(str::to_owned);
        let (dir, window, form) = open(cx, move |window, cx| {
            let mut form = CommitForm::new(repository_id(), data, window, cx);
            form.set_head(repository_id(), staged, last_commit, cx);
            form
        });
        Form {
            _dir: dir,
            window,
            form,
            fakes,
        }
    }

    impl Form {
        fn click(&self, id: &'static str, cx: &mut TestAppContext) {
            cx.update_window(self.window, |_, window, cx| window.click(id, cx))
                .unwrap();
            cx.run_until_parked();
        }

        /// Types `text` in the summary, at its end; `replace` first selects what's there.
        fn type_summary(&self, text: &str, replace: bool, cx: &mut TestAppContext) {
            let summary = self
                .form
                .read_with(cx, |form, cx| form.editor.read(cx).summary().clone());
            cx.update_window(self.window, |_, window, cx| {
                summary.update(cx, |input, cx| input.focus(window, cx));
                window.render_frame(cx);
                window.press(if replace { "secondary-a" } else { "end" }, cx);
                window.input(text, cx);
            })
            .unwrap();
            cx.run_until_parked();
        }

        /// Moves to the end of a multi-line input: there's no key for it every platform shares.
        #[cfg(target_os = "macos")]
        const TO_END: &str = "cmd-down";
        #[cfg(not(target_os = "macos"))]
        const TO_END: &str = "ctrl-end";

        fn type_description(&self, text: &str, cx: &mut TestAppContext) {
            let description = self
                .form
                .read_with(cx, |form, cx| form.editor.read(cx).description().clone());
            cx.update_window(self.window, |_, window, cx| {
                description.update(cx, |input, cx| input.focus(window, cx));
                window.render_frame(cx);
                window.press(Self::TO_END, cx);
                window.input(text, cx);
            })
            .unwrap();
            cx.run_until_parked();
        }

        fn set_head(
            &self,
            repository_id: RepositoryId,
            staged: usize,
            sha: Option<&str>,
            cx: &mut TestAppContext,
        ) {
            let sha = sha.map(str::to_owned);
            self.form
                .update(cx, |form, cx| form.set_head(repository_id, staged, sha, cx));
            cx.run_until_parked();
        }

        /// What the fields show: the summary, and the description.
        fn fields(&self, cx: &mut TestAppContext) -> (String, String) {
            self.render(cx);
            self.form.read_with(cx, |form, cx| {
                let editor = form.editor.read(cx);
                (
                    editor.summary().read(cx).value().to_string(),
                    editor.description().read(cx).value().to_string(),
                )
            })
        }

        fn is_readonly(&self, cx: &mut TestAppContext) -> bool {
            self.render(cx);
            self.form.read_with(cx, |form, cx| {
                !form.editor.read(cx).summary().read(cx).is_editable()
            })
        }

        fn is_amend_checked(&self, cx: &mut TestAppContext) -> bool {
            self.render(cx);
            cx.update_window(self.window, |_, window, _| window.find("amend").checked())
                .unwrap()
                == Some(true)
        }

        fn is_amend_disabled(&self, cx: &mut TestAppContext) -> bool {
            self.render(cx);
            cx.update_window(self.window, |_, window, _| window.find("amend").disabled())
                .unwrap()
                == Some(true)
        }

        /// The commit button's label, and whether it's enabled.
        fn button(&self, cx: &mut TestAppContext) -> (String, bool) {
            self.render(cx);
            let disabled = cx
                .update_window(self.window, |_, window, _| window.find("commit").disabled())
                .unwrap();
            let label = self.form.read_with(cx, |form, _| form.label());
            (label, disabled != Some(true))
        }

        fn render(&self, cx: &mut TestAppContext) {
            cx.update_window(self.window, |_, window, cx| window.render_frame(cx))
                .unwrap();
        }

        fn placeholder(&self, cx: &mut TestAppContext) -> &'static str {
            self.form
                .read_with(cx, |form, cx| form.editor.read(cx).placeholder())
        }

        fn committed(&self) -> Vec<NewCommit> {
            self.fakes.committed.borrow().clone()
        }
    }

    fn amended(message: &str, sha: &str) -> NewCommit {
        NewCommit {
            message: message.to_owned(),
            amend: Some(sha.to_owned()),
        }
    }

    #[gpui_kit::test]
    fn edits_the_last_commits_message_and_amends_it_without_anything_staged(
        cx: &mut TestAppContext,
    ) {
        let form = render_form(cx, 0, Some("a1"));
        assert_eq!(form.button(cx), ("Nothing staged".to_owned(), false));

        form.click("amend", cx);
        assert_eq!(
            form.fields(cx),
            ("Commit a1".to_owned(), "Its body".to_owned())
        );

        form.type_summary("Reworded", true, cx);
        assert_eq!(form.button(cx), ("Amend message".to_owned(), true));
        form.click("commit", cx);
        assert_eq!(form.committed(), [amended("Reworded\n\nIts body\n", "a1")]);
        assert!(!form.is_amend_checked(cx));
    }

    /// Holds the form without rendering it.
    struct Unrendered {
        form: Entity<CommitForm>,
    }

    impl Render for Unrendered {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    #[gpui_kit::test]
    fn puts_the_last_commits_message_in_the_fields_without_being_rendered(cx: &mut TestAppContext) {
        let fakes = Fakes::new();
        let data = fakes.data();
        let _dir = crate::test_support::init(cx, |services| services);
        let (_, host) = crate::test_support::open(cx, move |window, cx| Unrendered {
            form: cx.new(|cx| {
                let mut form = CommitForm::new(repository_id(), data, window, cx);
                form.set_head(repository_id(), 0, Some("a1".to_owned()), cx);
                form
            }),
        });
        let form = host.read_with(cx, |host, _| host.form.clone());
        form.update(cx, |form, cx| form.toggle_amend(true, cx));
        cx.run_until_parked();
        assert_eq!(
            form.read_with(cx, |form, cx| {
                let editor = form.editor.read(cx);
                (
                    editor.summary().read(cx).value().to_string(),
                    editor.description().read(cx).value().to_string(),
                )
            }),
            ("Commit a1".to_owned(), "Its body".to_owned())
        );
    }

    #[gpui_kit::test]
    fn amends_what_wasnt_edited_exactly_as_it_was_written(cx: &mut TestAppContext) {
        let form = render_form(cx, 1, Some("a1"));
        form.fakes
            .messages
            .once(Ok("First line\nsecond line\n\n    code();\n".into()));
        form.click("amend", cx);
        assert_eq!(form.button(cx), ("Amend with 1 file".to_owned(), true));
        form.click("commit", cx);
        assert_eq!(
            form.committed().last(),
            Some(&amended("First line\nsecond line\n\n    code();\n", "a1"))
        );

        form.set_head(repository_id(), 1, Some("b2"), cx);
        form.fakes
            .messages
            .once(Ok("First line\nsecond line\n\nBody\n".into()));
        form.click("amend", cx);
        assert_eq!(form.fields(cx).1, "Body");
        form.type_description(" edited", cx);
        form.click("commit", cx);
        assert_eq!(
            form.committed().last(),
            Some(&amended("First line\nsecond line\n\nBody edited\n", "b2"))
        );
    }

    #[gpui_kit::test]
    fn cant_be_edited_until_the_last_commits_message_has_loaded(cx: &mut TestAppContext) {
        let form = render_form(cx, 0, Some("a1"));
        let answer = form.fakes.messages.later();
        form.click("amend", cx);
        assert!(form.is_readonly(cx));
        assert_eq!(form.placeholder(cx), "Loading the last commit…");

        answer.send(Ok("Loaded".into())).unwrap();
        cx.run_until_parked();
        assert_eq!(form.fields(cx).0, "Loaded");
        assert!(!form.is_readonly(cx));
    }

    #[gpui_kit::test]
    fn keeps_both_messages_when_switching_between_them_and_the_new_one_after_amending(
        cx: &mut TestAppContext,
    ) {
        let form = render_form(cx, 2, Some("a1"));
        form.type_summary("Draft", false, cx);

        form.click("amend", cx);
        assert_eq!(form.fields(cx).0, "Commit a1");
        form.type_summary(" edited", false, cx);
        form.click("amend", cx);
        assert_eq!(form.fields(cx).0, "Draft");
        form.click("amend", cx);
        assert_eq!(form.fields(cx).0, "Commit a1 edited");

        assert_eq!(form.button(cx), ("Amend with 2 files".to_owned(), true));
        form.click("commit", cx);
        assert_eq!(form.fields(cx).0, "Draft");
    }

    #[gpui_kit::test]
    fn stops_amending_once_head_moves_on_without_loading_the_new_commits_message(
        cx: &mut TestAppContext,
    ) {
        let form = render_form(cx, 1, Some("a1"));
        form.type_summary("Draft", false, cx);
        form.click("amend", cx);
        assert_eq!(form.fields(cx).0, "Commit a1");

        // Made in a terminal, say.
        form.set_head(repository_id(), 1, Some("b2"), cx);
        assert!(!form.is_amend_checked(cx));
        assert_eq!(form.fields(cx).0, "Draft");
        assert!(!form.fakes.messages.calls().contains(&"b2".to_owned()));
    }

    #[gpui_kit::test]
    fn stops_amending_once_the_amend_moves_head_before_its_done(cx: &mut TestAppContext) {
        let form = render_form(cx, 1, Some("a1"));
        form.click("amend", cx);
        assert_eq!(form.fields(cx).0, "Commit a1");
        let finish = form.fakes.commits.later();
        form.click("commit", cx);
        assert!(form.is_readonly(cx));

        form.set_head(repository_id(), 1, Some("b2"), cx);
        finish.send(Ok(())).unwrap();
        cx.run_until_parked();
        assert!(!form.is_amend_checked(cx));
        assert_eq!(form.fakes.messages.calls().len(), 1);
    }

    #[gpui_kit::test]
    fn turns_off_when_theres_no_commit_left_to_amend_and_stays_off(cx: &mut TestAppContext) {
        let form = render_form(cx, 1, Some("a1"));
        form.click("amend", cx);
        assert_eq!(form.fields(cx).0, "Commit a1");

        form.set_head(repository_id(), 1, None, cx);
        assert!(!form.is_amend_checked(cx));
        assert!(form.is_amend_disabled(cx));
        form.type_summary("First commit", false, cx);
        assert_eq!(form.button(cx), ("Commit 1 file".to_owned(), true));

        form.set_head(repository_id(), 1, Some("c3"), cx);
        assert!(!form.is_amend_checked(cx));
        assert_eq!(form.button(cx), ("Commit 1 file".to_owned(), true));
    }

    #[gpui_kit::test]
    fn warns_when_the_last_commit_is_already_on_a_remote_branch(cx: &mut TestAppContext) {
        let form = render_form(cx, 0, Some("a1"));
        form.fakes.pushed_to.once(Ok(Some("origin/side".into())));
        form.click("amend", cx);
        let warning = form.form.read_with(cx, |form, _| form.push_warning());
        assert!(warning.unwrap().contains("already on origin/side"));
    }

    #[gpui_kit::test]
    fn doesnt_come_back_on_when_head_returns_to_the_commit(cx: &mut TestAppContext) {
        let form = render_form(cx, 0, Some("a1"));
        form.click("amend", cx);
        assert_eq!(form.fields(cx).0, "Commit a1");

        form.set_head(repository_id(), 0, Some("b2"), cx);
        form.set_head(repository_id(), 0, Some("a1"), cx);
        assert!(!form.is_amend_checked(cx));
        assert_eq!(form.fields(cx).0, "");
    }

    #[gpui_kit::test]
    fn says_why_the_fields_stay_locked_when_the_last_commits_message_cant_be_loaded(
        cx: &mut TestAppContext,
    ) {
        let form = render_form(cx, 0, Some("a1"));
        form.fakes.messages.once(Err(QueryError::msg("log failed")));
        form.click("amend", cx);
        let error = form.form.read_with(cx, |form, cx| form.error(cx));
        assert_eq!(error.unwrap().message(), "log failed");
        assert!(form.is_readonly(cx));
        assert_eq!(form.placeholder(cx), "Summary of your changes");

        form.click("amend", cx);
        assert!(!form.is_readonly(cx));
    }

    #[gpui_kit::test]
    fn waits_for_the_push_check_before_amending_so_its_warning_is_seen(cx: &mut TestAppContext) {
        let form = render_form(cx, 1, Some("a1"));
        let finish = form.fakes.pushed_to.later();
        form.click("amend", cx);
        assert_eq!(form.fields(cx).0, "Commit a1");
        assert_eq!(form.button(cx), ("Amend with 1 file".to_owned(), false));

        finish.send(Ok(Some("origin/feature".into()))).unwrap();
        cx.run_until_parked();
        let warning = form.form.read_with(cx, |form, _| form.push_warning());
        assert!(warning.unwrap().contains("already on origin/feature"));
        assert_eq!(form.button(cx), ("Amend with 1 file".to_owned(), true));
    }

    #[gpui_kit::test]
    fn doesnt_amend_with_nothing_staged_and_the_same_message(cx: &mut TestAppContext) {
        let form = render_form(cx, 0, Some("a1"));
        form.click("amend", cx);
        assert_eq!(form.fields(cx).0, "Commit a1");
        assert_eq!(form.button(cx), ("Amend message".to_owned(), false));

        form.type_summary(" ", false, cx);
        assert_eq!(form.button(cx), ("Amend message".to_owned(), false));
        form.type_summary("!", false, cx);
        assert_eq!(form.button(cx), ("Amend message".to_owned(), true));
    }

    #[gpui_kit::test]
    fn doesnt_carry_amending_over_to_another_repository_at_the_same_commit(
        cx: &mut TestAppContext,
    ) {
        let form = render_form(cx, 1, Some("a1"));
        form.click("amend", cx);
        assert_eq!(form.fields(cx).0, "Commit a1");

        form.set_head(other_repository_id(), 1, Some("a1"), cx);
        assert!(!form.is_amend_checked(cx));
        assert_eq!(form.fields(cx).0, "");
    }

    #[gpui_kit::test]
    fn doesnt_call_a_failed_push_check_an_error_but_says_it_couldnt_check(cx: &mut TestAppContext) {
        let form = render_form(cx, 1, Some("a1"));
        form.fakes
            .pushed_to
            .once(Err(QueryError::msg("for-each-ref failed")));
        form.click("amend", cx);
        assert_eq!(form.fields(cx).0, "Commit a1");
        assert_eq!(form.fakes.pushed_to.calls(), ["a1"]);
        assert!(form.form.read_with(cx, |form, cx| form.error(cx)).is_none());
        let warning = form.form.read_with(cx, |form, _| form.push_warning());
        assert!(
            warning
                .unwrap()
                .starts_with("Couldn't check whether the last commit has been pushed")
        );
        assert_eq!(form.button(cx), ("Amend with 1 file".to_owned(), true));
    }

    #[gpui_kit::test]
    fn cant_amend_before_the_first_commit(cx: &mut TestAppContext) {
        let form = render_form(cx, 0, None);
        assert!(form.is_amend_disabled(cx));
    }

    #[gpui_kit::test]
    fn commits_with_the_shortcut_from_either_field(cx: &mut TestAppContext) {
        let form = render_form(cx, 1, Some("a1"));
        form.type_summary("Shortcut", false, cx);
        form.type_description("Body", cx);
        cx.update_window(form.window, |_, window, cx| {
            window.press("secondary-enter", cx)
        })
        .unwrap();
        cx.run_until_parked();
        assert_eq!(
            form.committed(),
            [NewCommit {
                message: "Shortcut\n\nBody".into(),
                amend: None,
            }]
        );
        // The draft's cleared, without a line added to the description first.
        assert_eq!(form.fields(cx), (String::new(), String::new()));
    }
}
