use std::rc::Rc;

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AnyWindowHandle, TestAppContext};

use super::*;
use crate::components::repository::details::test_fakes::{
    Fakes, open_with, other_repository_id, repository_id,
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
    let last_commit = last_commit.map(str::to_owned);
    let (dir, window, form) = open_with(cx, &fakes, move |window, cx| {
        let mut form = CommitForm::new(repository_id(), window, cx);
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
        let summary = self.form.read_with(cx, |form, _| form.summary.clone());
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
        let description = self.form.read_with(cx, |form, _| form.description.clone());
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
            (
                form.summary.read(cx).value().to_string(),
                form.description.read(cx).value().to_string(),
            )
        })
    }

    fn is_readonly(&self, cx: &mut TestAppContext) -> bool {
        self.render(cx);
        self.form
            .read_with(cx, |form, cx| !form.summary.read(cx).is_editable())
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
fn edits_the_last_commits_message_and_amends_it_without_anything_staged(cx: &mut TestAppContext) {
    let form = render_form(cx, 0, Some("a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"));
    assert_eq!(form.button(cx), ("Nothing staged".to_owned(), false));

    form.click("amend", cx);
    assert_eq!(
        form.fields(cx),
        (
            "Commit a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1".to_owned(),
            "Its body".to_owned()
        )
    );

    form.type_summary("Reworded", true, cx);
    assert_eq!(form.button(cx), ("Amend message".to_owned(), true));
    form.click("commit", cx);
    assert_eq!(
        form.committed(),
        [amended(
            "Reworded\n\nIts body\n",
            "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"
        )]
    );
    assert!(!form.is_amend_checked(cx));
}

#[gpui_kit::test]
fn amends_what_wasnt_edited_exactly_as_it_was_written(cx: &mut TestAppContext) {
    let form = render_form(cx, 1, Some("a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"));
    form.fakes
        .messages
        .once(Ok("First line\nsecond line\n\n    code();\n".into()));
    form.click("amend", cx);
    assert_eq!(form.button(cx), ("Amend with 1 file".to_owned(), true));
    form.click("commit", cx);
    assert_eq!(
        form.committed().last(),
        Some(&amended(
            "First line\nsecond line\n\n    code();\n",
            "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"
        ))
    );

    form.set_head(
        repository_id(),
        1,
        Some("b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2"),
        cx,
    );
    form.fakes
        .messages
        .once(Ok("First line\nsecond line\n\nBody\n".into()));
    form.click("amend", cx);
    assert_eq!(form.fields(cx).1, "Body");
    form.type_description(" edited", cx);
    form.click("commit", cx);
    assert_eq!(
        form.committed().last(),
        Some(&amended(
            "First line\nsecond line\n\nBody edited\n",
            "b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2"
        ))
    );
}

#[gpui_kit::test]
fn cant_be_edited_until_the_last_commits_message_has_loaded(cx: &mut TestAppContext) {
    let form = render_form(cx, 0, Some("a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"));
    let answer = form.fakes.messages.later();
    form.click("amend", cx);
    assert!(form.is_readonly(cx));
    assert_eq!(
        form.form.read_with(cx, |form, _| form.placeholder),
        "Loading the last commit…"
    );

    answer.send(Ok("Loaded".into())).unwrap();
    cx.run_until_parked();
    assert_eq!(form.fields(cx).0, "Loaded");
    assert!(!form.is_readonly(cx));
}

#[gpui_kit::test]
fn keeps_both_messages_when_switching_between_them_and_the_new_one_after_amending(
    cx: &mut TestAppContext,
) {
    let form = render_form(cx, 2, Some("a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"));
    form.type_summary("Draft", false, cx);

    form.click("amend", cx);
    assert_eq!(
        form.fields(cx).0,
        "Commit a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"
    );
    form.type_summary(" edited", false, cx);
    form.click("amend", cx);
    assert_eq!(form.fields(cx).0, "Draft");
    form.click("amend", cx);
    assert_eq!(
        form.fields(cx).0,
        "Commit a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1 edited"
    );

    assert_eq!(form.button(cx), ("Amend with 2 files".to_owned(), true));
    form.click("commit", cx);
    assert_eq!(form.fields(cx).0, "Draft");
}

#[gpui_kit::test]
fn stops_amending_once_head_moves_on_without_loading_the_new_commits_message(
    cx: &mut TestAppContext,
) {
    let form = render_form(cx, 1, Some("a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"));
    form.type_summary("Draft", false, cx);
    form.click("amend", cx);
    assert_eq!(
        form.fields(cx).0,
        "Commit a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"
    );

    // Made in a terminal, say.
    form.set_head(
        repository_id(),
        1,
        Some("b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2"),
        cx,
    );
    assert!(!form.is_amend_checked(cx));
    assert_eq!(form.fields(cx).0, "Draft");
    assert!(
        !form
            .fakes
            .messages
            .calls()
            .contains(&"b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2".to_owned())
    );
}

#[gpui_kit::test]
fn stops_amending_once_the_amend_moves_head_before_its_done(cx: &mut TestAppContext) {
    let form = render_form(cx, 1, Some("a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"));
    form.click("amend", cx);
    assert_eq!(
        form.fields(cx).0,
        "Commit a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"
    );
    let finish = form.fakes.commits.later();
    form.click("commit", cx);
    assert!(form.is_readonly(cx));

    form.set_head(
        repository_id(),
        1,
        Some("b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2"),
        cx,
    );
    finish.send(Ok(())).unwrap();
    cx.run_until_parked();
    assert!(!form.is_amend_checked(cx));
    assert_eq!(form.fakes.messages.calls().len(), 1);
}

#[gpui_kit::test]
fn turns_off_when_theres_no_commit_left_to_amend_and_stays_off(cx: &mut TestAppContext) {
    let form = render_form(cx, 1, Some("a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"));
    form.click("amend", cx);
    assert_eq!(
        form.fields(cx).0,
        "Commit a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"
    );

    form.set_head(repository_id(), 1, None, cx);
    assert!(!form.is_amend_checked(cx));
    assert!(form.is_amend_disabled(cx));
    form.type_summary("First commit", false, cx);
    assert_eq!(form.button(cx), ("Commit 1 file".to_owned(), true));

    form.set_head(
        repository_id(),
        1,
        Some("c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3"),
        cx,
    );
    assert!(!form.is_amend_checked(cx));
    assert_eq!(form.button(cx), ("Commit 1 file".to_owned(), true));
}

#[gpui_kit::test]
fn warns_when_the_last_commit_is_already_on_a_remote_branch(cx: &mut TestAppContext) {
    let form = render_form(cx, 0, Some("a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"));
    form.fakes.pushed_to.once(Ok(Some("origin/side".into())));
    form.click("amend", cx);
    let warning = form.form.read_with(cx, |form, cx| form.push_warning(cx));
    assert!(warning.unwrap().contains("already on origin/side"));
}

#[gpui_kit::test]
fn doesnt_come_back_on_when_head_returns_to_the_commit(cx: &mut TestAppContext) {
    let form = render_form(cx, 0, Some("a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"));
    form.click("amend", cx);
    assert_eq!(
        form.fields(cx).0,
        "Commit a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"
    );

    form.set_head(
        repository_id(),
        0,
        Some("b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2"),
        cx,
    );
    form.set_head(
        repository_id(),
        0,
        Some("a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"),
        cx,
    );
    assert!(!form.is_amend_checked(cx));
    assert_eq!(form.fields(cx).0, "");
}

#[gpui_kit::test]
fn says_why_the_fields_stay_locked_when_the_last_commits_message_cant_be_loaded(
    cx: &mut TestAppContext,
) {
    let form = render_form(cx, 0, Some("a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"));
    form.fakes.messages.once(Err(DataError::msg("log failed")));
    form.click("amend", cx);
    let error = form.form.read_with(cx, |form, cx| form.error(cx));
    assert_eq!(error.unwrap().message(), "log failed");
    assert!(form.is_readonly(cx));
    assert_eq!(
        form.form.read_with(cx, |form, _| form.placeholder),
        "Summary of your changes"
    );

    form.click("amend", cx);
    assert!(!form.is_readonly(cx));
}

#[gpui_kit::test]
fn waits_for_the_push_check_before_amending_so_its_warning_is_seen(cx: &mut TestAppContext) {
    let form = render_form(cx, 1, Some("a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"));
    let finish = form.fakes.pushed_to.later();
    form.click("amend", cx);
    assert_eq!(
        form.fields(cx).0,
        "Commit a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"
    );
    assert_eq!(form.button(cx), ("Amend with 1 file".to_owned(), false));

    finish.send(Ok(Some("origin/feature".into()))).unwrap();
    cx.run_until_parked();
    let warning = form.form.read_with(cx, |form, cx| form.push_warning(cx));
    assert!(warning.unwrap().contains("already on origin/feature"));
    assert_eq!(form.button(cx), ("Amend with 1 file".to_owned(), true));
}

#[gpui_kit::test]
fn doesnt_amend_with_nothing_staged_and_the_same_message(cx: &mut TestAppContext) {
    let form = render_form(cx, 0, Some("a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"));
    form.click("amend", cx);
    assert_eq!(
        form.fields(cx).0,
        "Commit a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"
    );
    assert_eq!(form.button(cx), ("Amend message".to_owned(), false));

    form.type_summary(" ", false, cx);
    assert_eq!(form.button(cx), ("Amend message".to_owned(), false));
    form.type_summary("!", false, cx);
    assert_eq!(form.button(cx), ("Amend message".to_owned(), true));
}

#[gpui_kit::test]
fn doesnt_carry_amending_over_to_another_repository_at_the_same_commit(cx: &mut TestAppContext) {
    let form = render_form(cx, 1, Some("a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"));
    form.click("amend", cx);
    assert_eq!(
        form.fields(cx).0,
        "Commit a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"
    );

    form.set_head(
        other_repository_id(),
        1,
        Some("a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"),
        cx,
    );
    assert!(!form.is_amend_checked(cx));
    assert_eq!(form.fields(cx).0, "");
}

#[gpui_kit::test]
fn doesnt_call_a_failed_push_check_an_error_but_says_it_couldnt_check(cx: &mut TestAppContext) {
    let form = render_form(cx, 1, Some("a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"));
    form.fakes
        .pushed_to
        .once(Err(DataError::msg("for-each-ref failed")));
    form.click("amend", cx);
    assert_eq!(
        form.fields(cx).0,
        "Commit a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"
    );
    assert_eq!(
        form.fakes.pushed_to.calls(),
        ["a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"]
    );
    assert!(form.form.read_with(cx, |form, cx| form.error(cx)).is_none());
    let warning = form.form.read_with(cx, |form, cx| form.push_warning(cx));
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
    let form = render_form(cx, 1, Some("a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"));
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
