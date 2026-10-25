use std::rc::Rc;

use futures::channel::oneshot;
use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::schema::FileStatus;
use gpui_kit::component::WindowExt as _;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AnyWindowHandle, MouseButton, TestAppContext};

use super::*;
use crate::components::repository::details::test_fakes::{
    Fakes, click_label, file, open_with, repository_id,
};
use gitto_git::features::operation::schema::Operation;

use gitto_app_data::data::Repositories;

struct Details {
    _dir: tempfile::TempDir,
    window: AnyWindowHandle,
    details: Entity<WorkingTreeDetails>,
    fakes: Rc<Fakes>,
}

fn modified(path: &str) -> ChangedFile {
    file(path, FileStatus::Modified, None)
}

/// The uncommitted changes' details, of the changes `fakes` has.
fn render_details(cx: &mut TestAppContext, fakes: Rc<Fakes>) -> Details {
    let (dir, window, details) = open_with(cx, &fakes, move |window, cx| {
        WorkingTreeDetails::new(&repository_id(), window, cx)
    });
    Details {
        _dir: dir,
        window,
        details,
        fakes,
    }
}

fn with_changes(unstaged: Vec<ChangedFile>, staged: Vec<ChangedFile>) -> Rc<Fakes> {
    let fakes = Fakes::new();
    fakes.set_changes(unstaged, staged);
    fakes
}

impl Details {
    /// Right-clicks the file `path` in the list `list`, which opens its menu.
    fn open_menu(&self, list: &'static str, path: &str, cx: &mut TestAppContext) {
        let id = SharedString::from(format!("file:{path}"));
        cx.update_window(self.window, |_, window, cx| {
            window.render_frame(cx);
            window.within(list).right_click(id, cx);
        })
        .unwrap();
        cx.run_until_parked();
    }

    /// Clicks the menu's "Discard changes…", which asks about discarding them.
    fn choose_discard(&self, cx: &mut TestAppContext) {
        cx.update_window(self.window, |_, window, cx| {
            click_label(window, "Discard changes…", MouseButton::Left, cx)
        })
        .unwrap();
        cx.run_until_parked();
    }

    fn click(&self, id: &'static str, cx: &mut TestAppContext) {
        cx.update_window(self.window, |_, window, cx| {
            window.render_frame(cx);
            window.click(id, cx);
        })
        .unwrap();
        cx.run_until_parked();
    }

    /// What the open dialog says discarding does; `None` without one.
    fn asked(&self, cx: &mut TestAppContext) -> Option<String> {
        let open = cx
            .update_window(self.window, |_, window, cx| window.has_active_dialog(cx))
            .unwrap();
        let prompt = self
            .details
            .read_with(cx, |details, cx| details.discard_prompt(cx));
        open.then(|| prompt.map(|prompt| prompt.describe()))
            .flatten()
    }

    fn press(&self, key: &str, cx: &mut TestAppContext) {
        cx.update_window(self.window, |_, window, cx| window.press(key, cx))
            .unwrap();
        cx.run_until_parked();
    }

    /// The Discard all button's label, and whether it's disabled.
    fn discard_all(&self, cx: &mut TestAppContext) -> (String, bool) {
        cx.update_window(self.window, |_, window, cx| {
            window.render_frame(cx);
            let button = window.find("discard-all");
            (
                button.label().unwrap_or_default().to_owned(),
                button.disabled() == Some(true),
            )
        })
        .unwrap()
    }

    fn discarded(&self) -> Vec<DiscardTarget> {
        self.fakes.discarded.borrow().clone()
    }

    fn reload(&self, cx: &mut TestAppContext) {
        cx.update(|cx| {
            let repository = Repositories::get(&repository_id(), cx).unwrap();
            repository.update(cx, |repository, cx| drop(repository.reload_uncommitted(cx)));
        });
        cx.run_until_parked();
    }

    fn counts(&self, cx: &mut TestAppContext) -> String {
        cx.update_window(self.window, |_, window, cx| {
            window.render_frame(cx);
            window
                .find("working-tree-counts")
                .label()
                .unwrap_or_default()
                .to_owned()
        })
        .unwrap()
    }
}

fn unstaged_file(file: ChangedFile) -> DiscardTarget {
    DiscardTarget::File {
        side: DiscardSide::Unstaged,
        file,
    }
}

// There's no Suspense to take the details off the page while the status reloads: they're
// the same, with the same lists, while it does, and say so once it's taken a while.
#[gpui_kit::test]
fn stay_on_the_page_while_the_status_is_refetched_so_their_lists_keep_their_scroll_position(
    cx: &mut TestAppContext,
) {
    let details = render_details(
        cx,
        with_changes(vec![modified("a.txt"), modified("b.txt")], vec![]),
    );
    assert_eq!(details.counts(cx), "On main · 0 staged, 2 unstaged");

    // A file saved in an editor.
    details.fakes.set_changes(
        vec![modified("a.txt"), modified("b.txt"), modified("c.txt")],
        vec![],
    );
    let (let_through, gate) = oneshot::channel();
    *details.fakes.status_gate.borrow_mut() = Some(gate);
    details.reload(cx);
    assert_eq!(details.counts(cx), "On main · 0 staged, 2 unstaged");
    cx.executor().advance_clock(DELAY);
    cx.run_until_parked();
    assert!(
        details
            .details
            .read_with(cx, |details, cx| details.updating.read(cx).is_on())
    );

    let_through.send(()).unwrap();
    cx.run_until_parked();
    assert_eq!(details.counts(cx), "On main · 0 staged, 3 unstaged");
    assert!(
        !details
            .details
            .read_with(cx, |details, cx| details.updating.read(cx).is_on())
    );
}

#[gpui_kit::test]
fn discards_a_files_unstaged_changes_from_its_menu_once_asked(cx: &mut TestAppContext) {
    let details = render_details(
        cx,
        with_changes(vec![modified("src/a.txt"), modified("b.txt")], vec![]),
    );

    details.open_menu("unstaged-files", "src/a.txt", cx);
    details.choose_discard(cx);
    let asked = details.asked(cx).unwrap();
    assert_eq!(asked, "The unstaged changes to src/a.txt are lost.");
    // Nothing of it is staged to keep.
    assert!(!asked.contains("staged of it"));
    assert!(details.discarded().is_empty());

    details.click("discard-confirm", cx);
    assert_eq!(details.discarded(), [unstaged_file(modified("src/a.txt"))]);
}

#[gpui_kit::test]
fn says_what_an_unstaged_changes_file_is_put_back_as_whats_staged_or_the_last_commit(
    cx: &mut TestAppContext,
) {
    let details = render_details(
        cx,
        with_changes(
            vec![modified("a.txt"), file("d.txt", FileStatus::Deleted, None)],
            vec![modified("a.txt")],
        ),
    );

    details.open_menu("unstaged-files", "a.txt", cx);
    details.choose_discard(cx);
    assert_eq!(
        details.asked(cx).unwrap(),
        "The unstaged changes to a.txt are lost. What's staged of it is kept."
    );
    details.press("escape", cx);
    assert_eq!(details.asked(cx), None);

    details.open_menu("unstaged-files", "d.txt", cx);
    details.choose_discard(cx);
    assert_eq!(
        details.asked(cx).unwrap(),
        "d.txt is put back as the last commit has it."
    );
    assert!(details.discarded().is_empty());
}

#[gpui_kit::test]
fn says_an_untracked_file_is_deleted_and_puts_a_staged_one_back_as_head_has_it(
    cx: &mut TestAppContext,
) {
    let details = render_details(
        cx,
        with_changes(
            vec![file("new.txt", FileStatus::Untracked, None)],
            vec![modified("staged.txt")],
        ),
    );

    details.open_menu("unstaged-files", "new.txt", cx);
    details.choose_discard(cx);
    assert_eq!(
        details.asked(cx).unwrap(),
        "new.txt isn't tracked, so it's deleted."
    );
    details.click("discard-cancel", cx);
    assert_eq!(details.asked(cx), None);

    details.open_menu("staged-files", "staged.txt", cx);
    details.choose_discard(cx);
    details.click("discard-confirm", cx);
    assert_eq!(
        details.discarded(),
        [DiscardTarget::File {
            side: DiscardSide::Staged,
            file: modified("staged.txt"),
        }]
    );
}

#[gpui_kit::test]
fn doesnt_discard_a_conflicted_files_changes_nor_all_of_them_while_there_are_conflicts(
    cx: &mut TestAppContext,
) {
    let details = render_details(
        cx,
        with_changes(
            vec![
                file("both.txt", FileStatus::Conflicted, None),
                modified("a.txt"),
            ],
            vec![],
        ),
    );

    details.open_menu("unstaged-files", "both.txt", cx);
    // Disabled: choosing it asks nothing.
    details.choose_discard(cx);
    assert_eq!(details.asked(cx), None);
    assert!(
        details
            .details
            .read_with(cx, |details, _| details.discarding.is_none())
    );
    details.press("escape", cx);
    assert_eq!(
        details.discard_all(cx),
        (
            "Resolve the conflicts before discarding all changes".to_owned(),
            true
        )
    );
}

#[gpui_kit::test]
fn discards_all_changes_once_asked(cx: &mut TestAppContext) {
    let details = render_details(
        cx,
        with_changes(vec![modified("a.txt")], vec![modified("b.txt")]),
    );

    assert_eq!(
        details.discard_all(cx),
        ("Discard all changes…".to_owned(), false)
    );
    details.click("discard-all", cx);
    assert!(
        details
            .asked(cx)
            .unwrap()
            .contains("untracked files are deleted")
    );
    details.click("discard-confirm", cx);
    assert_eq!(details.discarded(), [DiscardTarget::All]);
}

#[gpui_kit::test]
fn cant_discard_all_changes_when_there_are_none_but_repositories_inside_this_one(
    cx: &mut TestAppContext,
) {
    let details = render_details(
        cx,
        with_changes(
            vec![file("vendor/lib/", FileStatus::Untracked, None)],
            vec![],
        ),
    );
    assert_eq!(details.counts(cx), "On main · 0 staged, 1 unstaged");
    assert_eq!(
        details.discard_all(cx),
        ("No changes to discard".to_owned(), true)
    );
}

#[gpui_kit::test]
fn cant_discard_all_changes_while_an_operation_is_under_way(cx: &mut TestAppContext) {
    let fakes = with_changes(vec![modified("a.txt")], vec![]);
    *fakes.operation.borrow_mut() = Some(Operation::Merge {
        merging: "side".into(),
        into: Some("main".into()),
    });
    let details = render_details(cx, fakes);
    assert_eq!(
        details.discard_all(cx),
        (
            "Finish or abort the merge before discarding all changes".to_owned(),
            true
        )
    );
}

#[gpui_kit::test]
fn says_a_renamed_file_is_put_back_at_its_old_path_and_a_copy_deleted(cx: &mut TestAppContext) {
    let renamed = file("new.txt", FileStatus::Renamed, Some("old.txt"));
    let copied = file("copy.txt", FileStatus::Copied, Some("lib.txt"));
    let details = render_details(
        cx,
        with_changes(vec![], vec![renamed.clone(), copied.clone()]),
    );

    details.open_menu("staged-files", "new.txt", cx);
    details.choose_discard(cx);
    assert!(
        details
            .asked(cx)
            .unwrap()
            .contains("new.txt is deleted, and old.txt put back as the last commit has it")
    );
    details.click("discard-confirm", cx);
    let staged = |file: ChangedFile| DiscardTarget::File {
        side: DiscardSide::Staged,
        file,
    };
    assert_eq!(details.discarded().last(), Some(&staged(renamed)));

    details.open_menu("staged-files", "copy.txt", cx);
    details.choose_discard(cx);
    assert!(
        details
            .asked(cx)
            .unwrap()
            .contains("copy.txt is new, so it's deleted")
    );
    details.click("discard-confirm", cx);
    assert_eq!(details.discarded().last(), Some(&staged(copied)));
}

#[gpui_kit::test]
fn shows_why_the_latest_action_failed_and_no_longer_once_another_goes_through(
    cx: &mut TestAppContext,
) {
    let details = render_details(
        cx,
        with_changes(vec![modified("a.txt"), modified("b.txt")], vec![]),
    );
    details
        .fakes
        .discards
        .once(Err(DataError::msg("Couldn't discard it.")));

    details.open_menu("unstaged-files", "a.txt", cx);
    details.choose_discard(cx);
    details.click("discard-confirm", cx);
    let error = details
        .details
        .read_with(cx, |details, cx| details.error(cx));
    assert_eq!(error.unwrap().message(), "Couldn't discard it.");

    cx.update_window(details.window, |_, window, cx| {
        window.render_frame(cx);
        click_label(window, "Stage b.txt", MouseButton::Left, cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        details.fakes.staged.calls(),
        [format!("{:?}", StagingTarget::Paths(vec!["b.txt".into()]))]
    );
    assert!(
        details
            .details
            .read_with(cx, |details, cx| details.error(cx))
            .is_none()
    );
}

#[gpui_kit::test]
fn asks_about_a_file_as_its_list_has_it_now_and_no_longer_once_its_gone(cx: &mut TestAppContext) {
    let details = render_details(
        cx,
        with_changes(vec![modified("a.txt"), modified("b.txt")], vec![]),
    );

    details.open_menu("unstaged-files", "a.txt", cx);
    details.choose_discard(cx);
    assert!(
        details
            .asked(cx)
            .unwrap()
            .contains("unstaged changes to a.txt")
    );

    // Untracked in a terminal: discarding it deletes it now.
    details.fakes.set_changes(
        vec![
            file("a.txt", FileStatus::Untracked, None),
            modified("b.txt"),
        ],
        vec![],
    );
    details.reload(cx);
    assert!(details.asked(cx).unwrap().contains("a.txt isn't tracked"));

    details.fakes.set_changes(vec![modified("b.txt")], vec![]);
    details.reload(cx);
    assert_eq!(details.asked(cx), None);
    let open = cx
        .update_window(details.window, |_, window, cx| window.has_active_dialog(cx))
        .unwrap();
    assert!(!open);
    assert!(details.discarded().is_empty());
}
