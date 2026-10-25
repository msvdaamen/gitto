use gitto_git::features::conflicts::schema::ConflictSides;
use gpui_kit::{AppContext as _, TestAppContext};

use super::super::super::test_views::{
    Opened, conflicted, id, init, open, reload_conflict_as, save, set_changes, set_conflict, side,
    text_conflict,
};
use super::super::conflict_viewer::ConflictProgress;
use super::*;
use crate::components::ui::{ConfirmDialog, Tone};
use crate::data::Repositories;

const MARKERS: &str = "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> side\n";

/// `count` conflicts, each after a numbered line.
fn conflicts(count: usize) -> String {
    (1..=count)
        .map(|line| format!("line {line}\n{MARKERS}"))
        .collect()
}

fn unstaged(file: &str) -> FileDiffEvent {
    FileDiffEvent::Open {
        source: DiffSource::Unstaged,
        file: conflicted(file),
    }
}

/// `a.txt` with `contents` (and `b.txt` conflicted too), open.
fn open_conflict(contents: &str, cx: &mut TestAppContext) -> Opened {
    set_changes(vec![], vec![conflicted("a.txt"), conflicted("b.txt")], cx);
    set_conflict("a.txt", text_conflict(contents, "v1"), cx);
    set_conflict("b.txt", text_conflict(MARKERS, "v1"), cx);
    open(DiffSource::Unstaged, conflicted("a.txt"), cx)
}

/// What the header says of the conflicts left, if it does.
fn left(opened: &Opened, cx: &mut TestAppContext) -> Option<String> {
    opened.read(cx, |view, _| {
        view.shown_progress()
            .filter(|shown| shown.problem.is_none())
            .map(|shown| conflicts_left_label(shown.left))
    })
}

fn progress(opened: &Opened, cx: &mut TestAppContext) -> Option<ConflictProgress> {
    opened.read(cx, |view, _| view.shown_progress().cloned())
}

fn shown_text(opened: &Opened, cx: &mut TestAppContext) -> Option<String> {
    opened.read(cx, |view, cx| {
        let viewer = view.resolution.viewer()?;
        viewer.read(cx).shown_text().map(str::to_owned)
    })
}

fn error(opened: &Opened, cx: &mut TestAppContext) -> Option<String> {
    opened.read(cx, |view, _| {
        view.resolution
            .error
            .as_ref()
            .filter(|(key, _)| *key == view.view.file_key())
            .map(|(_, message)| message.clone())
    })
}

fn mark(version: &str, with_markers: bool) -> FileToResolve {
    FileToResolve {
        repository_id: id(),
        path: "a.txt".into(),
        resolution: ConflictResolution::Mark {
            version: Some(version.into()),
            with_markers,
        },
    }
}

// A file with text conflicts.

#[gpui_kit::test]
fn shows_its_conflicts_rather_than_a_patch_and_how_many_are_left(cx: &mut TestAppContext) {
    let _dir = init(cx);
    let opened = open_conflict(&conflicts(2), cx);

    assert_eq!(shown_text(&opened, cx), Some(conflicts(2)));
    assert_eq!(left(&opened, cx).as_deref(), Some("2 conflicts left"));
    // "Changed on both sides", with a side to keep whole.
    assert!(opened.has("keep-all-ours", cx));
    let loaded = cx.update(|cx| {
        let repository = Repositories::open(&id(), cx).read(cx);
        repository
            .patch(&DiffSource::Unstaged, "a.txt")
            .is_some_and(|patch| patch.is_fetching() || patch.data().is_some())
    });
    assert!(!loaded);
    // Ours above theirs, one way only.
    assert!(!opened.has("split", cx));
    assert!(!opened.has("stage-file", cx));
}

#[gpui_kit::test]
fn is_only_marked_resolved_with_conflicts_left_once_the_user_says_they_belong_in_it(
    cx: &mut TestAppContext,
) {
    let _dir = init(cx);
    let opened = open_conflict(&conflicts(2), cx);

    opened.press("ctrl-enter", cx);
    assert!(opened.has_dialog(cx));
    opened.click("cancel", cx);
    assert!(!opened.has_dialog(cx));
    assert_eq!(opened.resolve.calls(), []);

    opened.click("mark-resolved", cx);
    assert!(opened.has_dialog(cx));
    opened.click(("action", 0usize), cx);
    assert_eq!(opened.resolve.calls(), [mark("v1", true)]);
    assert_eq!(opened.events(), [unstaged("b.txt")]);
}

#[gpui_kit::test]
fn doesnt_take_another_files_progress_for_its_own(cx: &mut TestAppContext) {
    let _dir = init(cx);
    let opened = open_conflict("resolved\n", cx);
    assert_eq!(left(&opened, cx).as_deref(), Some("No conflicts left"));

    // The viewer still says how the last file went, while this one's conflicts are highlighted.
    opened.view.update(cx, |view, cx| {
        view.on_conflict_viewer(
            &ConflictViewerEvent::Progress(ConflictProgress {
                file_key: Some(gitto_ui_logic::git::diff_source::diff_file_key(
                    &DiffSource::Unstaged,
                    "b.txt",
                )),
                left: 0,
                current: None,
                saving: false,
                version: Some("v9".into()),
                problem: None,
            }),
            cx,
        )
    });
    assert!(!opened.is_enabled("mark-resolved", cx));
    assert_ne!(left(&opened, cx).as_deref(), Some("No conflicts left"));
}

#[gpui_kit::test]
fn leaves_its_keys_to_a_dialog_or_menu_on_top_of_it(cx: &mut TestAppContext) {
    let _dir = init(cx);
    let opened = open_conflict(&conflicts(2), cx);
    cx.update_window(opened.window, |_, window, cx| {
        ConfirmDialog::new(IconName::TriangleAlert, Tone::Amber, "A dialog").open(window, cx)
    })
    .unwrap();
    cx.run_until_parked();

    opened.press("t", cx);
    opened.press("]", cx);
    assert_eq!(opened.tree.saves(), []);
    assert_eq!(
        progress(&opened, cx).and_then(|shown| shown.current),
        Some(1)
    );
}

#[gpui_kit::test]
fn is_marked_resolved_once_none_are_then_the_next_conflicted_file_opens(cx: &mut TestAppContext) {
    let _dir = init(cx);
    let opened = open_conflict("resolved\n", cx);
    assert_eq!(left(&opened, cx).as_deref(), Some("No conflicts left"));

    opened.click("mark-resolved", cx);
    assert_eq!(opened.resolve.calls(), [mark("v1", false)]);
    assert_eq!(opened.events(), [unstaged("b.txt")]);
}

#[gpui_kit::test]
fn opens_its_staged_changes_once_it_was_the_last_conflicted_file(cx: &mut TestAppContext) {
    let _dir = init(cx);
    set_changes(vec![], vec![conflicted("a.txt")], cx);
    set_conflict("a.txt", text_conflict("resolved\n", "v1"), cx);
    let opened = open(DiffSource::Unstaged, conflicted("a.txt"), cx);
    assert_eq!(left(&opened, cx).as_deref(), Some("No conflicts left"));

    opened.press("ctrl-enter", cx);
    assert_eq!(
        opened.events(),
        [FileDiffEvent::Open {
            source: DiffSource::Staged,
            file: ChangedFile {
                status: FileStatus::Modified,
                ..conflicted("a.txt")
            },
        }]
    );
}

#[gpui_kit::test]
fn says_why_it_couldnt_be_marked_resolved(cx: &mut TestAppContext) {
    let _dir = init(cx);
    let opened = open_conflict("resolved\n", cx);
    opened.resolve.fail_once(
        "a.txt still has conflict markers. Resolve its conflicts, then mark it resolved.",
    );

    opened.click("mark-resolved", cx);
    assert_eq!(
        error(&opened, cx).as_deref(),
        Some("a.txt still has conflict markers. Resolve its conflicts, then mark it resolved.")
    );
    assert_eq!(opened.events(), []);
}

#[gpui_kit::test]
fn steps_through_its_conflicts_and_resolves_them_with_the_keyboard(cx: &mut TestAppContext) {
    let _dir = init(cx);
    let opened = open_conflict(&conflicts(3), cx);
    let current = |cx: &mut TestAppContext| progress(&opened, cx).and_then(|p| p.current);
    assert_eq!(current(cx), Some(1));

    opened.press("]", cx);
    assert_eq!(current(cx), Some(2));
    opened.press("[", cx);
    opened.press("[", cx);
    // Round to the other end.
    assert_eq!(current(cx), Some(3));

    // Ours of the third, then on to the second: the one before, as it was the last.
    opened.press("o", cx);
    let third = conflicts(3).replace(&format!("line 3\n{MARKERS}"), "line 3\nours\n");
    assert_eq!(opened.tree.saves(), [save("a.txt", &third, "v1", false)]);
    assert_eq!(current(cx), Some(2));
    opened.press("t", cx);
    let second = third.replace(&format!("line 2\n{MARKERS}"), "line 2\ntheirs\n");
    assert_eq!(
        opened.tree.saves().last(),
        Some(&save("a.txt", &second, "v2", false))
    );
    opened.press("b", cx);
    let both = second.replace(&format!("line 1\n{MARKERS}"), "line 1\nours\ntheirs\n");
    assert_eq!(
        opened.tree.saves().last(),
        Some(&save("a.txt", &both, "v2", false))
    );
    assert_eq!(left(&opened, cx).as_deref(), Some("No conflicts left"));

    opened.press("n", cx);
    assert_eq!(opened.events(), [unstaged("b.txt")]);
}

#[gpui_kit::test]
fn writes_a_resolved_conflict_over_the_version_it_was_read_at(cx: &mut TestAppContext) {
    let _dir = init(cx);
    let opened = open_conflict(MARKERS, cx);

    opened.click(("keep-ours", 0usize), cx);
    assert_eq!(opened.tree.saves(), [save("a.txt", "ours\n", "v1", false)]);
    assert_eq!(
        progress(&opened, cx).and_then(|p| p.version).as_deref(),
        Some("v2")
    );
}

#[gpui_kit::test]
fn says_why_a_conflict_couldnt_be_resolved(cx: &mut TestAppContext) {
    let _dir = init(cx);
    let opened = open_conflict(MARKERS, cx);
    opened.tree.reply(|_| {
        Err(DataError::msg(
            "a.txt was changed on disk since you started editing it.",
        ))
    });

    opened.click(("keep-theirs", 0usize), cx);
    assert_eq!(
        error(&opened, cx).as_deref(),
        Some("a.txt was changed on disk since you started editing it.")
    );
    // Read again, to be shown as it is now.
    assert_eq!(opened.tree.reloads(), [Reload::Conflict("a.txt".into())]);
    // Still as it was.
    assert_eq!(left(&opened, cx).as_deref(), Some("1 conflict left"));
}

#[gpui_kit::test]
fn keeps_a_side_whole_of_the_version_shown(cx: &mut TestAppContext) {
    let _dir = init(cx);
    let opened = open_conflict(&conflicts(2), cx);

    opened.click("keep-all-theirs", cx);
    assert_eq!(
        opened.resolve.calls(),
        [FileToResolve {
            repository_id: id(),
            path: "a.txt".into(),
            resolution: ConflictResolution::Keep {
                side: Side::Theirs,
                sides: ConflictSides {
                    base: Some(side("b1")),
                    ours: Some(side("o1")),
                    theirs: Some(side("t1")),
                },
                version: Some("v1".into()),
            },
        }]
    );
    assert_eq!(opened.events(), [unstaged("b.txt")]);
}

#[gpui_kit::test]
fn isnt_resolved_whole_while_its_edited_as_the_edits_arent_known_yet(cx: &mut TestAppContext) {
    let _dir = init(cx);
    let opened = open_conflict("resolved\n", cx);
    assert_eq!(left(&opened, cx).as_deref(), Some("No conflicts left"));

    opened.click("edit-file", cx);
    assert!(opened.read(cx, |view, _| view.editing.is_editing()));
    assert!(!opened.is_enabled("mark-resolved", cx));
    assert!(!opened.is_enabled("keep-all-ours", cx));
    opened.press("ctrl-enter", cx);
    assert_eq!(opened.resolve.calls(), []);
}

#[gpui_kit::test]
fn is_edited_by_hand_and_read_again_once_that_stops_before_its_shown(cx: &mut TestAppContext) {
    let _dir = init(cx);
    let opened = open_conflict(&conflicts(2), cx);

    opened.click("edit-file", cx);
    assert!(opened.read(cx, |view, _| view.editing.is_editing()));
    // What's read while it's edited isn't for the viewer.
    assert_eq!(opened.read(cx, |view, _| view.disk()), None);
    assert!(opened.read(cx, |view, cx| {
        view.resolution
            .viewer()
            .is_some_and(|viewer| viewer.read(cx).whole_file().is_some())
    }));

    reload_conflict_as("a.txt", text_conflict("resolved\n", "v3"), cx);
    opened.click("edit-file", cx);
    assert_eq!(
        opened.read(cx, |view, _| view.disk()),
        Some(DiskFile {
            contents: "resolved\n".into(),
            version: "v3".into(),
        })
    );
    assert_eq!(shown_text(&opened, cx).as_deref(), Some("resolved\n"));
    assert_eq!(
        progress(&opened, cx).and_then(|p| p.version).as_deref(),
        Some("v3")
    );
}

// A file that isn't text on both sides.

#[gpui_kit::test]
fn says_what_each_side_did_and_keeps_one_whole(cx: &mut TestAppContext) {
    let _dir = init(cx);
    set_changes(vec![], vec![conflicted("gone.txt")], cx);
    let conflict = Conflict {
        theirs: None,
        ..text_conflict("ours\n", "v1")
    };
    assert_eq!(
        describe_conflict(&conflict.sides()),
        "Deleted in theirs, changed in ours"
    );
    set_conflict("gone.txt", conflict, cx);
    let opened = open(DiffSource::Unstaged, conflicted("gone.txt"), cx);

    opened.click("keep-theirs", cx);
    let calls = opened.resolve.calls();
    assert!(matches!(
        &calls[..],
        [FileToResolve {
            path,
            resolution: ConflictResolution::Keep {
                side: Side::Theirs,
                version: Some(version),
                ..
            },
            ..
        }] if path == "gone.txt" && version == "v1"
    ));
    // It was the last one: its staged changes, its deletion, are next.
    assert!(matches!(
        &opened.events()[..],
        [FileDiffEvent::Open { source: DiffSource::Staged, file }] if file.path == "gone.txt"
    ));
    assert!(!opened.has("mark-resolved", cx));
}

#[gpui_kit::test]
fn says_a_binary_file_cant_be_shown_as_text(cx: &mut TestAppContext) {
    let _dir = init(cx);
    set_changes(vec![], vec![conflicted("gone.txt")], cx);
    set_conflict(
        "gone.txt",
        Conflict {
            text: None,
            binary: true,
            unreadable: Some("This file is binary.".into()),
            ..text_conflict(MARKERS, "v1")
        },
        cx,
    );
    let opened = open(DiffSource::Unstaged, conflicted("gone.txt"), cx);

    assert!(opened.is_enabled("keep-ours", cx));
    assert!(opened.is_enabled("keep-theirs", cx));
    assert!(!opened.has("mark-resolved", cx));
}
