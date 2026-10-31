use std::cell::RefCell;
use std::collections::VecDeque;

use futures::channel::oneshot;
use gitto_git::PatchVersion;
use gitto_git::features::staging::schema::{LineRange, LineSelection};
use gitto_git::features::status::schema::{
    Head, StatusCounts, StatusSummary, Uncommitted, WorkingTreeFiles,
};
use gitto_ui_logic::git::changes::staging_paths;
use gpui_kit::component::WindowExt as _;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, TestAppContext};

use super::super::shown_file::cache_name;
use super::super::test_views::{DiffGit, init, set_patch_in, set_status, update_repository};
use super::super::viewer::line_picker::LineStaging;
use super::super::viewer::viewer_runtime::ViewerRuntime;
use super::*;
use crate::components::repository::test_support::repository_entity;
use gitto_app_data::data::diff::{ChangedFiles, FilesOf};
use gitto_app_data::data::staging::{FileToStage, LinesToStage};
use gitto_app_data::data::{DataError, Loaded, Mutation, Opaque};

fn id() -> RepositoryId {
    RepositoryId::parse("01890a5d-ac96-774b-bcce-b302099a8057").unwrap()
}

fn file(path: &str, additions: Option<u64>, deletions: Option<u64>) -> ChangedFile {
    ChangedFile {
        path: path.into(),
        status: FileStatus::Modified,
        orig_path: None,
        additions,
        deletions,
    }
}

fn counted(path: &str) -> ChangedFile {
    file(path, Some(1), Some(0))
}

fn with_status(file: ChangedFile, status: FileStatus) -> ChangedFile {
    ChangedFile { status, ..file }
}

const SHA: &str = "a1a1a1a";

fn commit() -> DiffSource {
    DiffSource::Commit { sha: SHA.into() }
}

fn files() -> Vec<ChangedFile> {
    vec![counted("a.txt"), counted("b.txt"), counted("c.txt")]
}

/// A patch of `path` with one line changed to `line`, as git would give it.
fn changed_line(path: &str, line: &str) -> String {
    format!(
        "diff --git a/{path} b/{path}\nindex 1111111..2222222 100644\n--- a/{path}\n+++ b/{path}\n@@ -1 +1 @@\n-old\n+{line}\n"
    )
}

/// Gives `file` of `source` the patch `patch`, as if it was loaded.
fn set_patch(source: &DiffSource, file: &ChangedFile, patch: &str, cx: &mut TestAppContext) {
    cx.update(|cx| set_patch_in(source, file, patch, cx));
}

/// Starts loading `file`'s patch in `source`, to be given by the sender returned: until then,
/// it's loading.
fn pending_patch(
    source: &DiffSource,
    file: &ChangedFile,
    cx: &mut TestAppContext,
) -> oneshot::Sender<FilePatch> {
    let key = DiffFileKey::new(source, &file.path);
    let send = cx.update(|cx| DiffGit::get(cx).later(key));
    update_repository(cx, |repository, cx| {
        drop(repository.fetch_patch(source, file, cx))
    });
    send
}

/// The commit's files, and each one's patch.
fn commit_files(cx: &mut TestAppContext) {
    set_commit_files(cx);
    patches(&commit(), &files(), "new", cx);
}

/// The commit's files, without their patches.
fn set_commit_files(cx: &mut TestAppContext) {
    let files = ChangedFiles {
        sha: SHA.into(),
        files: Opaque::new(files()),
    };
    update_repository(cx, |repository, cx| {
        repository.set_files(FilesOf::Commit, files, cx)
    });
}

/// The uncommitted changes the status says there are.
fn set_changes(
    staged: Vec<ChangedFile>,
    unstaged: Vec<ChangedFile>,
    uncounted: bool,
    cx: &mut TestAppContext,
) {
    let status = Uncommitted {
        summary: StatusSummary {
            head: Head::Branch {
                name: "main".into(),
                sha: "a1".into(),
            },
            upstream: None,
            ahead: 0,
            behind: 0,
            counts: StatusCounts {
                files: 0,
                staged: 0,
                unstaged: 0,
                conflicted: 0,
            },
        },
        changes: WorkingTreeFiles {
            staged,
            unstaged,
            uncounted,
            marker_free: Vec::new(),
        },
        version: uuid::Uuid::new_v4().to_string().into(),
    };
    set_status(status, cx);
}

/// Gives each of `files` of `source` a patch changing its line to `<prefix> <path>`.
fn patches(source: &DiffSource, files: &[ChangedFile], prefix: &str, cx: &mut TestAppContext) {
    for file in files {
        set_patch(
            source,
            file,
            &changed_line(&file.path, &format!("{prefix} {}", file.path)),
            cx,
        );
    }
}

type Respond<I, R> = Box<dyn FnMut(&I, &mut App) -> Task<Result<R, DataError>>>;

/// A mutation a test makes up: it records its inputs, and does what `respond` says.
struct Fake<I: 'static, R: 'static> {
    mutation: Entity<Mutation<I, R>>,
    calls: Rc<RefCell<Vec<I>>>,
    respond: Rc<RefCell<Respond<I, R>>>,
}

impl<I: Clone + 'static, R: 'static> Fake<I, R> {
    fn new(cx: &mut TestAppContext) -> Self {
        let calls: Rc<RefCell<Vec<I>>> = Rc::default();
        let respond: Rc<RefCell<Respond<I, R>>> = Rc::new(RefCell::new(Box::new(|_, _| {
            Task::ready(Err(DataError::msg("No reply set.")))
        })));
        let mutation = cx.update(|cx| {
            let calls = Rc::clone(&calls);
            let respond = Rc::clone(&respond);
            Mutation::new(
                move |input: I, cx: &mut App| {
                    calls.borrow_mut().push(input.clone());
                    (respond.borrow_mut())(&input, cx)
                },
                cx,
            )
        });
        Fake {
            mutation,
            calls,
            respond,
        }
    }

    fn respond(&self, respond: impl FnMut(&I, &mut App) -> Task<Result<R, DataError>> + 'static) {
        *self.respond.borrow_mut() = Box::new(respond);
    }

    fn calls(&self) -> Vec<I> {
        self.calls.borrow().clone()
    }
}

/// A file's changes, open, with what they asked of the page.
struct Opened {
    window: AnyWindowHandle,
    view: Entity<FileDiffView>,
    events: Rc<RefCell<Vec<FileDiffEvent>>>,
    lines: Fake<LinesToStage, String>,
    whole: Fake<FileToStage, ()>,
}

impl Opened {
    /// The patch on show.
    fn shown(&self, cx: &mut TestAppContext) -> Option<String> {
        cx.update(|cx| {
            let viewer = self.view.read(cx).viewer()?.clone();
            viewer.read(cx).shown_patch().map(str::to_owned)
        })
    }

    /// The file the header names.
    fn header(&self, cx: &mut TestAppContext) -> String {
        cx.update(|cx| self.view.read(cx).view.shown().file.path)
    }

    fn body(&self, cx: &mut TestAppContext) -> Body {
        cx.update(|cx| self.view.read(cx).body())
    }

    fn staging(&self, cx: &mut TestAppContext) -> Option<LineStaging> {
        cx.update(|cx| self.view.read(cx).line_staging(cx))
    }

    fn events(&self) -> Vec<FileDiffEvent> {
        self.events.borrow().clone()
    }

    fn staging_error(&self, cx: &mut TestAppContext) -> Option<String> {
        cx.update(|cx| {
            let view = self.view.read(cx);
            view.staging
                .error_for(&view.view.file_key())
                .map(str::to_owned)
        })
    }

    fn set_file(&self, source: DiffSource, file: ChangedFile, cx: &mut TestAppContext) {
        self.view
            .update(cx, |view, cx| view.set_file(id(), source, file, cx));
        cx.run_until_parked();
    }

    fn click(&self, element: &'static str, cx: &mut TestAppContext) {
        cx.update_window(self.window, |_, window, cx| {
            window.render_frame(cx);
            window.click(element, cx);
        })
        .unwrap();
        cx.run_until_parked();
    }

    fn press(&self, key: &'static str, cx: &mut TestAppContext) {
        cx.update_window(self.window, |_, window, cx| window.press(key, cx))
            .unwrap();
        cx.run_until_parked();
    }

    fn is_enabled(&self, element: &'static str, cx: &mut TestAppContext) -> bool {
        cx.update_window(self.window, |_, window, cx| {
            window.render_frame(cx);
            !window.find(element).disabled().unwrap_or(false)
        })
        .unwrap()
    }

    fn has(&self, element: &'static str, cx: &mut TestAppContext) -> bool {
        cx.update_window(self.window, |_, window, cx| {
            window.render_frame(cx);
            window.try_find(element).is_some()
        })
        .unwrap()
    }

    /// Stages `lines` of `patch` as the viewer would; what it resolved to, once it has.
    fn stage(
        &self,
        patch: &str,
        lines: LineSelection,
        cx: &mut TestAppContext,
    ) -> Rc<RefCell<Option<Result<(), DataError>>>> {
        assert!(self.staging(cx).is_some(), "lines can be staged");
        let result = Rc::new(RefCell::new(None));
        let done = Rc::clone(&result);
        let patch: Arc<str> = patch.into();
        cx.update(|cx| {
            let staged = self
                .view
                .update(cx, |view, cx| view.stage_lines(patch, lines, cx));
            cx.spawn(async move |_| *done.borrow_mut() = Some(staged.await))
                .detach();
        });
        cx.run_until_parked();
        result
    }
}

fn open(source: DiffSource, shown: ChangedFile, cx: &mut TestAppContext) -> Opened {
    let lines = Fake::new(cx);
    let whole = Fake::new(cx);
    let (stage_lines, stage_file) = (lines.mutation.clone(), whole.mutation.clone());
    let (window, view) = crate::test_support::open(cx, |window, cx| {
        FileDiffView::with_mutations(id(), source, shown, stage_lines, stage_file, window, cx)
    });
    let events: Rc<RefCell<Vec<FileDiffEvent>>> = Rc::default();
    let recorded = Rc::clone(&events);
    cx.update(|cx| {
        cx.subscribe(&view, move |_, event: &FileDiffEvent, _| {
            recorded.borrow_mut().push(event.clone())
        })
        .detach()
    });
    cx.run_until_parked();
    Opened {
        window,
        view,
        events,
        lines,
        whole,
    }
}

fn lines() -> LineSelection {
    LineSelection {
        deletions: vec![LineRange::new(1, 1).unwrap()],
        additions: vec![LineRange::new(1, 1).unwrap()],
    }
}

fn opened_with(source: DiffSource, file: ChangedFile) -> FileDiffEvent {
    FileDiffEvent::Open { source, file }
}

// A file's changes.

#[gpui_kit::test]
fn shows_the_files_patch_a_step_away_from_the_commits_other_files(cx: &mut TestAppContext) {
    let _dir = init(cx);
    commit_files(cx);
    let opened = open(commit(), counted("b.txt"), cx);

    assert_eq!(opened.shown(cx), Some(changed_line("b.txt", "new b.txt")));
    opened.click("next-file", cx);
    assert_eq!(opened.events(), [opened_with(commit(), counted("c.txt"))]);
    opened.click("previous-file", cx);
    assert_eq!(
        opened.events().last(),
        Some(&opened_with(commit(), counted("a.txt")))
    );
}

#[gpui_kit::test]
fn keeps_the_last_file_on_show_header_and_all_until_the_next_ones_changes_are_in(
    cx: &mut TestAppContext,
) {
    let _dir = init(cx);
    set_commit_files(cx);
    patches(&commit(), &[counted("a.txt"), counted("c.txt")], "new", cx);
    let next = pending_patch(&commit(), &counted("b.txt"), cx);
    let opened = open(commit(), counted("a.txt"), cx);
    assert_eq!(opened.shown(cx), Some(changed_line("a.txt", "new a.txt")));

    opened.set_file(commit(), counted("b.txt"), cx);
    assert_eq!(opened.shown(cx), Some(changed_line("a.txt", "new a.txt")));
    assert_eq!(opened.header(cx), "a.txt");

    next.send(FilePatch {
        key: DiffFileKey::new(&commit(), "b.txt"),
        file: counted("b.txt"),
        patch: changed_line("b.txt", "later").into(),
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(opened.shown(cx), Some(changed_line("b.txt", "later")));
    assert_eq!(opened.header(cx), "b.txt");
}

#[gpui_kit::test]
fn loads_the_files_a_step_away_ahead_once_its_on_show(cx: &mut TestAppContext) {
    let _dir = init(cx);
    commit_files(cx);
    let opened = open(commit(), counted("b.txt"), cx);
    assert!(opened.shown(cx).is_some());
    // Parsed and highlighted, named as they'll be when they're opened.
    cx.update(|cx| {
        for path in ["a.txt", "c.txt"] {
            let key = format!("{}:0", cache_name(&DiffFileKey::new(&commit(), path)));
            assert!(ViewerRuntime::has_highlights(&key, cx), "{path}");
        }
    });
}

#[gpui_kit::test]
fn cant_step_past_the_first_file(cx: &mut TestAppContext) {
    let _dir = init(cx);
    commit_files(cx);
    let opened = open(commit(), counted("a.txt"), cx);

    assert!(opened.shown(cx).is_some());
    assert!(!opened.is_enabled("previous-file", cx));
    assert!(opened.is_enabled("next-file", cx));
}

#[gpui_kit::test]
fn doesnt_load_a_binary_files_patch(cx: &mut TestAppContext) {
    let _dir = init(cx);
    commit_files(cx);
    let image = file("image.png", None, None);
    let opened = open(commit(), image.clone(), cx);

    assert_eq!(opened.body(cx), Body::Binary);
    let loading = cx.update(|cx| {
        repository_entity(&id(), cx)
            .read(cx)
            .patch(&commit(), &image.path)
            .is_some_and(Loaded::is_fetching)
    });
    assert!(!loading);
}

#[gpui_kit::test]
fn says_a_file_was_only_renamed(cx: &mut TestAppContext) {
    let _dir = init(cx);
    commit_files(cx);
    let renamed = ChangedFile {
        status: FileStatus::Renamed,
        orig_path: Some("old.txt".into()),
        ..file("new.txt", Some(0), Some(0))
    };
    let opened = open(commit(), renamed.clone(), cx);

    assert_eq!(opened.body(cx), Body::NoChangedLines);
    assert_eq!(
        unchanged_reason(&renamed),
        "Renamed from old.txt, with the same contents."
    );
}

#[gpui_kit::test]
fn asks_before_loading_a_large_change(cx: &mut TestAppContext) {
    let _dir = init(cx);
    commit_files(cx);
    let big = file("big.lock", Some(30_000), Some(0));
    set_patch(&commit(), &big, &changed_line("big.lock", "big"), cx);
    let opened = open(commit(), big, cx);

    assert_eq!(opened.body(cx), Body::Large(30_000));
    opened.click("show-large", cx);
    assert_eq!(opened.shown(cx), Some(changed_line("big.lock", "big")));
}

#[gpui_kit::test]
fn tries_the_next_file_afresh_after_one_couldnt_be_shown(cx: &mut TestAppContext) {
    let _dir = init(cx);
    commit_files(cx);
    let broken = counted("broken.txt");
    set_patch(&commit(), &broken, "patch of broken.txt", cx);
    let opened = open(commit(), broken, cx);

    assert_eq!(
        opened.body(cx),
        Body::Error("The patch has no file in it.".into())
    );
    opened.set_file(commit(), counted("b.txt"), cx);
    assert_eq!(opened.shown(cx), Some(changed_line("b.txt", "new b.txt")));
}

#[gpui_kit::test]
fn says_an_empty_file_was_added(cx: &mut TestAppContext) {
    let _dir = init(cx);
    commit_files(cx);
    let added = with_status(file(".gitkeep", Some(0), Some(0)), FileStatus::Added);
    let opened = open(commit(), added.clone(), cx);

    assert_eq!(opened.body(cx), Body::NoChangedLines);
    assert_eq!(unchanged_reason(&added), "An empty file was added.");
}

#[gpui_kit::test]
fn leaves_esc_to_a_dialog_that_closes_on_it(cx: &mut TestAppContext) {
    let _dir = init(cx);
    commit_files(cx);
    let opened = open(commit(), counted("a.txt"), cx);
    cx.update_window(opened.window, |_, window, cx| {
        window.open_dialog(cx, |dialog, _, _| dialog)
    })
    .unwrap();
    cx.run_until_parked();

    opened.press("escape", cx);
    assert_eq!(opened.events(), []);
    let open = cx
        .update_window(opened.window, |_, window, cx| window.has_active_dialog(cx))
        .unwrap();
    assert!(!open);
}

/// What opens a file's changes, to give the focus back to.
struct Host {
    opener: FocusHandle,
    view: Option<Entity<FileDiffView>>,
    _subscription: Option<Subscription>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(
                div()
                    .id("opener")
                    .track_focus(&self.opener)
                    .child("Open a.txt"),
            )
            .children(self.view.clone())
    }
}

#[gpui_kit::test]
fn gives_the_focus_back_to_what_opened_it_once_it_closes(cx: &mut TestAppContext) {
    let _dir = init(cx);
    commit_files(cx);
    let (window, host) = crate::test_support::open(cx, |_, cx| Host {
        opener: cx.focus_handle(),
        view: None,
        _subscription: None,
    });
    cx.update_window(window, |_, window, cx| {
        let opener = host.read(cx).opener.clone();
        window.focus(&opener, cx);
        host.update(cx, |host, cx| {
            let view = cx.new(|cx| {
                let lines = Mutation::new(|_, _| Task::ready(Ok(String::new())), cx);
                let whole = Mutation::new(|_, _| Task::ready(Ok(())), cx);
                FileDiffView::with_mutations(
                    id(),
                    commit(),
                    counted("a.txt"),
                    lines,
                    whole,
                    window,
                    cx,
                )
            });
            host._subscription = Some(cx.subscribe(&view, |host: &mut Host, _, event, cx| {
                if *event == FileDiffEvent::Close {
                    host.view = None;
                    cx.notify();
                }
            }));
            host.view = Some(view);
            cx.notify();
        });
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("close-file", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let focused = cx
        .update_window(window, |_, window, cx| {
            host.read(cx).opener.is_focused(window)
        })
        .unwrap();
    assert!(host.read_with(cx, |host, _| host.view.is_none()));
    assert!(focused);
}

#[gpui_kit::test]
fn goes_back_to_the_history_with_esc(cx: &mut TestAppContext) {
    let _dir = init(cx);
    commit_files(cx);
    let opened = open(commit(), counted("a.txt"), cx);

    assert!(opened.shown(cx).is_some());
    opened.press("escape", cx);
    assert_eq!(opened.events(), [FileDiffEvent::Close]);
}

// An uncommitted file's changes.

#[gpui_kit::test]
fn are_updated_in_place_when_they_change_on_disk(cx: &mut TestAppContext) {
    let _dir = init(cx);
    set_changes(vec![], vec![counted("a.txt")], false, cx);
    patches(&DiffSource::Unstaged, &[counted("a.txt")], "new", cx);
    let opened = open(DiffSource::Unstaged, counted("a.txt"), cx);
    assert_eq!(opened.shown(cx), Some(changed_line("a.txt", "new a.txt")));

    set_patch(
        &DiffSource::Unstaged,
        &counted("a.txt"),
        &changed_line("a.txt", "saved"),
        cx,
    );
    cx.run_until_parked();
    assert_eq!(opened.shown(cx), Some(changed_line("a.txt", "saved")));
}

#[gpui_kit::test]
fn prepares_a_patch_loaded_again_off_the_main_thread_keeping_the_last_meanwhile(
    cx: &mut TestAppContext,
) {
    let _dir = init(cx);
    set_changes(vec![], vec![counted("a.txt")], false, cx);
    patches(&DiffSource::Unstaged, &[counted("a.txt")], "new", cx);
    let opened = open(DiffSource::Unstaged, counted("a.txt"), cx);
    let prepared = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            opened
                .view
                .read(cx)
                .view
                .patch()
                .map(|prepared| (prepared.data.patch.to_string(), prepared.cache_key.clone()))
        })
    };
    let (_, before) = prepared(cx).expect("prepared");

    set_patch(
        &DiffSource::Unstaged,
        &counted("a.txt"),
        &changed_line("a.txt", "saved"),
        cx,
    );
    // Summed up and named (by a hash of all of it) in the background: not yet.
    assert_eq!(
        prepared(cx),
        Some((changed_line("a.txt", "new a.txt"), before.clone()))
    );
    cx.run_until_parked();
    let (patch, after) = prepared(cx).expect("prepared");
    assert_eq!(patch, changed_line("a.txt", "saved"));
    assert_ne!(after, before);
}

#[gpui_kit::test]
fn says_when_the_file_has_no_unstaged_changes_left_with_a_way_to_its_staged_ones(
    cx: &mut TestAppContext,
) {
    let _dir = init(cx);
    set_changes(vec![], vec![counted("a.txt")], false, cx);
    patches(&DiffSource::Unstaged, &[counted("a.txt")], "new", cx);
    let opened = open(DiffSource::Unstaged, counted("a.txt"), cx);
    assert!(opened.shown(cx).is_some());

    set_changes(vec![counted("a.txt")], vec![], false, cx);
    set_patch(&DiffSource::Unstaged, &counted("a.txt"), "", cx);
    cx.run_until_parked();
    assert_eq!(opened.body(cx), Body::NoLongerChanged);
    assert_eq!(opened.shown(cx), None);
    opened.click("show-other-side", cx);
    assert_eq!(
        opened.events(),
        [opened_with(DiffSource::Staged, counted("a.txt"))]
    );
}

#[gpui_kit::test]
fn tells_from_the_patch_whether_an_untracked_file_is_binary(cx: &mut TestAppContext) {
    let _dir = init(cx);
    let image = with_status(file("image.png", None, None), FileStatus::Untracked);
    set_changes(vec![], vec![image.clone()], false, cx);
    set_patch(
        &DiffSource::Unstaged,
        &image,
        "diff --git a/image.png b/image.png\nnew file mode 100644\nindex 0000000..1111111\nBinary files /dev/null and b/image.png differ\n",
        cx,
    );
    let opened = open(DiffSource::Unstaged, image, cx);

    assert_eq!(opened.body(cx), Body::Binary);
}

#[gpui_kit::test]
fn asks_before_showing_a_large_file_that_wasnt_counted(cx: &mut TestAppContext) {
    let _dir = init(cx);
    let log = file("huge.log", None, None);
    set_changes(vec![], vec![log.clone()], true, cx);
    let lines = "+line\n".repeat(25_000);
    set_patch(
        &DiffSource::Unstaged,
        &log,
        &format!(
            "diff --git a/huge.log b/huge.log\nindex 1..2 100644\n@@ -0,0 +1,25000 @@\n{lines}"
        ),
        cx,
    );
    let opened = open(DiffSource::Unstaged, log, cx);

    assert_eq!(opened.body(cx), Body::Large(25_000));
    assert_eq!(thousands(25_000), "25,000");
}

#[gpui_kit::test]
fn stays_on_the_page_while_theyre_refetched_so_the_view_keeps_its_scroll_position(
    cx: &mut TestAppContext,
) {
    let _dir = init(cx);
    set_changes(vec![], vec![counted("a.txt")], false, cx);
    patches(&DiffSource::Unstaged, &[counted("a.txt")], "new", cx);
    let opened = open(DiffSource::Unstaged, counted("a.txt"), cx);
    let viewer = cx
        .update(|cx| opened.view.read(cx).viewer().cloned())
        .unwrap();

    set_changes(vec![], vec![file("a.txt", Some(2), Some(1))], false, cx);
    set_patch(
        &DiffSource::Unstaged,
        &counted("a.txt"),
        &changed_line("a.txt", "saved"),
        cx,
    );
    cx.run_until_parked();
    assert_eq!(opened.shown(cx), Some(changed_line("a.txt", "saved")));
    let still = cx
        .update(|cx| opened.view.read(cx).viewer().cloned())
        .unwrap();
    assert_eq!(still.entity_id(), viewer.entity_id());
}

#[gpui_kit::test]
fn doesnt_step_from_where_another_file_was_for_one_that_isnt_in_its_list(cx: &mut TestAppContext) {
    let _dir = init(cx);
    let unstaged = vec![counted("a.txt"), counted("b.txt"), counted("c.txt")];
    set_changes(vec![counted("d.txt")], unstaged.clone(), false, cx);
    patches(&DiffSource::Unstaged, &unstaged, "new", cx);
    patches(
        &DiffSource::Staged,
        &[counted("d.txt"), counted("a.txt")],
        "staged",
        cx,
    );
    let opened = open(DiffSource::Unstaged, counted("b.txt"), cx);
    assert!(opened.is_enabled("next-file", cx));

    // Opened from the staged list as it was being unstaged.
    opened.set_file(DiffSource::Staged, counted("a.txt"), cx);
    assert_eq!(opened.body(cx), Body::NoLongerChanged);
    assert!(!opened.is_enabled("previous-file", cx));
    assert!(!opened.is_enabled("next-file", cx));
}

#[gpui_kit::test]
fn doesnt_load_a_file_without_line_counts_ahead_which_can_be_of_any_size(cx: &mut TestAppContext) {
    let _dir = init(cx);
    let untracked = |path| with_status(file(path, None, None), FileStatus::Untracked);
    let unstaged = vec![counted("a.txt"), untracked("b.log"), untracked("c.log")];
    set_changes(vec![], unstaged.clone(), false, cx);
    patches(&DiffSource::Unstaged, &unstaged, "new", cx);
    let opened = open(DiffSource::Unstaged, untracked("b.log"), cx);

    assert_eq!(opened.shown(cx), Some(changed_line("b.log", "new b.log")));
    cx.update(|cx| {
        let key = |path: &str| {
            let patch = changed_line(path, &format!("new {path}"));
            format!(
                "{}:{}:0",
                cache_name(&DiffFileKey::new(&DiffSource::Unstaged, path)),
                PatchVersion::of(&patch)
            )
        };
        assert!(ViewerRuntime::has_highlights(&key("a.txt"), cx));
        assert!(!ViewerRuntime::has_highlights(&key("c.log"), cx));
    });
}

// Staging lines.

/// What the data layer does once lines are staged: the patch they leave takes the place of
/// the one they were picked from, if it has changes left.
fn leaves(
    source: DiffSource,
    file: ChangedFile,
    left: String,
) -> impl FnMut(&LinesToStage, &mut App) -> Task<Result<String, DataError>> + 'static {
    move |_, cx| {
        if gitto_ui_logic::git::patch::has_hunks(&left) {
            set_patch_in(&source, &file, &left, cx);
        }
        Task::ready(Ok(left.clone()))
    }
}

#[gpui_kit::test]
fn stages_the_lines_picked_from_the_patch_on_show_and_shows_the_patch_they_leave(
    cx: &mut TestAppContext,
) {
    let _dir = init(cx);
    let unstaged = vec![counted("a.txt"), counted("b.txt")];
    set_changes(vec![], unstaged.clone(), false, cx);
    patches(&DiffSource::Unstaged, &unstaged, "new", cx);
    let opened = open(DiffSource::Unstaged, counted("a.txt"), cx);
    opened.lines.respond(leaves(
        DiffSource::Unstaged,
        counted("a.txt"),
        changed_line("a.txt", "left"),
    ));

    assert_eq!(opened.staging(cx).unwrap().action, StageAction::Stage);
    let from = changed_line("a.txt", "new a.txt");
    let result = opened.stage(&from, lines(), cx);
    assert!(matches!(*result.borrow(), Some(Ok(()))));
    let calls = opened.lines.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].file, counted("a.txt"));
    assert_eq!(calls[0].action, StageAction::Stage);
    assert_eq!(calls[0].version, PatchVersion::of(&from));
    assert_eq!(calls[0].lines, lines());
    assert_eq!(opened.shown(cx), Some(changed_line("a.txt", "left")));
}

/// The view in a pane narrower than its window, as the page has it between the sidebar and
/// the details.
struct Narrow(Entity<FileDiffView>);

impl Render for Narrow {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(div().w(gpui_kit::px(500.)).h_full().child(self.0.clone()))
    }
}

#[gpui_kit::test]
fn shows_the_hunks_buttons_in_a_pane_narrower_than_the_window_once_opened(cx: &mut TestAppContext) {
    let _dir = init(cx);
    set_changes(vec![], vec![counted("a.txt")], false, cx);
    patches(&DiffSource::Unstaged, &[counted("a.txt")], "new", cx);
    let (window, _) = crate::test_support::open(cx, |window, cx| {
        let lines = Mutation::new(
            |_: LinesToStage, _: &mut App| Task::ready(Ok(String::new())),
            cx,
        );
        let whole = Mutation::new(|_: FileToStage, _: &mut App| Task::ready(Ok(())), cx);
        let source = DiffSource::Unstaged;
        Narrow(cx.new(|cx| {
            FileDiffView::with_mutations(id(), source, counted("a.txt"), lines, whole, window, cx)
        }))
    });
    cx.run_until_parked();

    let right = cx
        .update_window(window, |_, window, _| {
            window.find(("stage-hunk", 0_usize)).bounds().right()
        })
        .unwrap();
    assert!(
        right <= gpui_kit::px(500.),
        "the button's right edge is at {right:?}"
    );
}

#[gpui_kit::test]
fn stages_the_lines_the_viewer_picks_with_the_keyboard(cx: &mut TestAppContext) {
    let _dir = init(cx);
    set_changes(vec![], vec![counted("a.txt")], false, cx);
    patches(&DiffSource::Unstaged, &[counted("a.txt")], "new", cx);
    let opened = open(DiffSource::Unstaged, counted("a.txt"), cx);
    opened.lines.respond(leaves(
        DiffSource::Unstaged,
        counted("a.txt"),
        changed_line("a.txt", "left"),
    ));

    let viewer = cx.update(|cx| opened.view.read(cx).viewer().cloned().unwrap());
    cx.update_window(opened.window, |_, window, cx| {
        window.focus(&viewer.focus_handle(cx), cx)
    })
    .unwrap();
    // The first changed line, the removed one, staged.
    opened.press("down", cx);
    opened.press("space", cx);
    let calls = opened.lines.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].version,
        PatchVersion::of(&changed_line("a.txt", "new a.txt"))
    );
    assert_eq!(
        calls[0].lines,
        LineSelection {
            deletions: vec![LineRange::new(1, 1).unwrap()],
            additions: vec![],
        }
    );
    assert_eq!(opened.shown(cx), Some(changed_line("a.txt", "left")));
}

#[gpui_kit::test]
fn stages_lines_of_an_untracked_file_as_such(cx: &mut TestAppContext) {
    let _dir = init(cx);
    let notes = with_status(file("notes.txt", None, None), FileStatus::Untracked);
    set_changes(vec![], vec![notes.clone()], false, cx);
    patches(
        &DiffSource::Unstaged,
        std::slice::from_ref(&notes),
        "new",
        cx,
    );
    let opened = open(DiffSource::Unstaged, notes.clone(), cx);
    opened.lines.respond(leaves(
        DiffSource::Unstaged,
        notes,
        changed_line("notes.txt", "rest"),
    ));

    opened.stage("the patch", lines(), cx);
    assert_eq!(opened.lines.calls()[0].file.status, FileStatus::Untracked);
}

#[gpui_kit::test]
fn unstages_lines_picked_from_the_staged_changes(cx: &mut TestAppContext) {
    let _dir = init(cx);
    set_changes(vec![counted("a.txt")], vec![], false, cx);
    patches(&DiffSource::Staged, &[counted("a.txt")], "staged", cx);
    let opened = open(DiffSource::Staged, counted("a.txt"), cx);
    assert_eq!(
        opened.shown(cx),
        Some(changed_line("a.txt", "staged a.txt"))
    );
    opened.lines.respond(leaves(
        DiffSource::Staged,
        counted("a.txt"),
        changed_line("a.txt", "still staged"),
    ));

    assert_eq!(opened.staging(cx).unwrap().action, StageAction::Unstage);
    opened.stage("the patch", lines(), cx);
    let calls = opened.lines.calls();
    assert_eq!(calls[0].action, StageAction::Unstage);
    assert_eq!(calls[0].version, PatchVersion::of("the patch"));
    assert_eq!(
        opened.shown(cx),
        Some(changed_line("a.txt", "still staged"))
    );
}

#[gpui_kit::test]
fn is_busy_until_the_patch_the_lines_leave_is_on_show(cx: &mut TestAppContext) {
    let _dir = init(cx);
    set_changes(vec![], vec![counted("a.txt")], false, cx);
    patches(&DiffSource::Unstaged, &[counted("a.txt")], "new", cx);
    let opened = open(DiffSource::Unstaged, counted("a.txt"), cx);
    let replies: Rc<RefCell<VecDeque<oneshot::Sender<String>>>> = Rc::default();
    let senders = Rc::clone(&replies);
    opened.lines.respond(move |_, cx| {
        let (send, receive) = oneshot::channel();
        senders.borrow_mut().push_back(send);
        cx.background_spawn(async move { receive.await.map_err(|_| DataError::msg("Dropped.")) })
    });

    assert!(!opened.staging(cx).unwrap().busy);
    let from = changed_line("a.txt", "new a.txt");
    let result = opened.stage(&from, lines(), cx);
    assert!(opened.staging(cx).unwrap().busy);
    let left = changed_line("a.txt", "left");
    set_patch(&DiffSource::Unstaged, &counted("a.txt"), &left, cx);
    replies
        .borrow_mut()
        .pop_front()
        .unwrap()
        .send(left)
        .unwrap();
    cx.run_until_parked();
    assert!(matches!(*result.borrow(), Some(Ok(()))));
    assert_eq!(opened.shown(cx), Some(changed_line("a.txt", "left")));
    assert!(!opened.staging(cx).unwrap().busy);
}

#[gpui_kit::test]
fn opens_the_next_file_once_the_last_lines_are_staged_this_ones_staying_on_show_till_then(
    cx: &mut TestAppContext,
) {
    let _dir = init(cx);
    let unstaged = vec![counted("a.txt"), counted("b.txt")];
    set_changes(vec![], unstaged.clone(), false, cx);
    patches(&DiffSource::Unstaged, &unstaged, "new", cx);
    let opened = open(DiffSource::Unstaged, counted("a.txt"), cx);
    opened.lines.respond(leaves(
        DiffSource::Unstaged,
        counted("a.txt"),
        String::new(),
    ));

    opened.stage(&changed_line("a.txt", "new a.txt"), lines(), cx);
    assert_eq!(
        opened.events(),
        [opened_with(DiffSource::Unstaged, counted("b.txt"))]
    );
    assert_ne!(opened.body(cx), Body::NoLongerChanged);
    assert_eq!(opened.shown(cx), Some(changed_line("a.txt", "new a.txt")));
}

#[gpui_kit::test]
fn opens_the_one_before_once_the_last_files_last_lines_are_staged(cx: &mut TestAppContext) {
    let _dir = init(cx);
    let unstaged = vec![counted("a.txt"), counted("b.txt")];
    set_changes(vec![], unstaged.clone(), false, cx);
    patches(&DiffSource::Unstaged, &unstaged, "new", cx);
    let opened = open(DiffSource::Unstaged, counted("b.txt"), cx);
    // Only its mode change is left.
    opened.lines.respond(leaves(
        DiffSource::Unstaged,
        counted("b.txt"),
        "diff --git a/b.txt b/b.txt\nold mode 100644\nnew mode 100755\n".into(),
    ));

    opened.stage(&changed_line("b.txt", "new b.txt"), lines(), cx);
    assert_eq!(
        opened.events(),
        [opened_with(DiffSource::Unstaged, counted("a.txt"))]
    );
}

#[gpui_kit::test]
fn says_why_lines_couldnt_be_staged_and_shows_the_changes_as_they_are_now(cx: &mut TestAppContext) {
    let _dir = init(cx);
    set_changes(vec![], vec![counted("a.txt")], false, cx);
    patches(&DiffSource::Unstaged, &[counted("a.txt")], "new", cx);
    let opened = open(DiffSource::Unstaged, counted("a.txt"), cx);
    let message = "The file changed since its changes were shown, so nothing was staged.";
    opened.lines.respond(move |_, cx| {
        // The changes are reloaded, as they are now.
        set_patch_in(
            &DiffSource::Unstaged,
            &counted("a.txt"),
            &changed_line("a.txt", "saved"),
            cx,
        );
        Task::ready(Err(DataError::msg(message)))
    });

    let result = opened.stage(&changed_line("a.txt", "new a.txt"), lines(), cx);
    let failed = result
        .borrow()
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap_err()
        .to_string();
    assert_eq!(failed, message);
    assert_eq!(opened.staging_error(cx).as_deref(), Some(message));
    assert_eq!(opened.shown(cx), Some(changed_line("a.txt", "saved")));
}

#[gpui_kit::test]
fn doesnt_pick_lines_of_a_commits_changes_nor_of_a_links(cx: &mut TestAppContext) {
    let _dir = init(cx);
    commit_files(cx);
    let opened = open(commit(), counted("a.txt"), cx);
    assert!(opened.shown(cx).is_some());
    assert!(opened.staging(cx).is_none());

    set_changes(vec![], vec![counted("link")], false, cx);
    set_patch(
        &DiffSource::Unstaged,
        &counted("link"),
        "diff --git a/link b/link\nindex 1..2 120000\n--- a/link\n+++ b/link\n@@ -1 +1 @@\n-a\n\\ No newline at end of file\n+b\n\\ No newline at end of file\n",
        cx,
    );
    let link = open(DiffSource::Unstaged, counted("link"), cx);
    assert!(link.shown(cx).is_some());
    assert!(link.staging(cx).is_none());
}

// Staging the whole file.

fn staged_whole() -> impl FnMut(&FileToStage, &mut App) -> Task<Result<(), DataError>> {
    |_, _| Task::ready(Ok(()))
}

#[gpui_kit::test]
fn stages_it_then_opens_the_next_file_in_the_list(cx: &mut TestAppContext) {
    let _dir = init(cx);
    let unstaged = vec![counted("a.txt"), counted("b.txt")];
    set_changes(vec![], unstaged.clone(), false, cx);
    patches(&DiffSource::Unstaged, &unstaged, "new", cx);
    let opened = open(DiffSource::Unstaged, counted("a.txt"), cx);
    opened.whole.respond(staged_whole());

    opened.click("stage-file", cx);
    let calls = opened.whole.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].action, StageAction::Stage);
    assert_eq!(staging_paths(&[calls[0].file.clone()]), ["a.txt"]);
    assert_eq!(
        opened.events(),
        [opened_with(DiffSource::Unstaged, counted("b.txt"))]
    );
}

#[gpui_kit::test]
fn stages_a_renamed_file_by_both_its_paths(cx: &mut TestAppContext) {
    let _dir = init(cx);
    let renamed = ChangedFile {
        status: FileStatus::Renamed,
        orig_path: Some("old.txt".into()),
        ..counted("new.txt")
    };
    set_changes(vec![], vec![renamed.clone()], false, cx);
    patches(
        &DiffSource::Unstaged,
        std::slice::from_ref(&renamed),
        "new",
        cx,
    );
    let opened = open(DiffSource::Unstaged, renamed, cx);
    opened.whole.respond(staged_whole());

    opened.click("stage-file", cx);
    assert_eq!(
        staging_paths(&[opened.whole.calls()[0].file.clone()]),
        ["new.txt", "old.txt"]
    );
}

#[gpui_kit::test]
fn unstages_a_staged_file_then_opens_the_one_before_the_last(cx: &mut TestAppContext) {
    let _dir = init(cx);
    let staged = vec![counted("a.txt"), counted("b.txt")];
    set_changes(staged.clone(), vec![], false, cx);
    patches(&DiffSource::Staged, &staged, "staged", cx);
    let opened = open(DiffSource::Staged, counted("b.txt"), cx);
    opened.whole.respond(staged_whole());

    opened.click("stage-file", cx);
    assert_eq!(opened.whole.calls()[0].action, StageAction::Unstage);
    assert_eq!(
        opened.events(),
        [opened_with(DiffSource::Staged, counted("a.txt"))]
    );
}

#[gpui_kit::test]
fn says_why_it_couldnt_be_staged(cx: &mut TestAppContext) {
    let _dir = init(cx);
    set_changes(vec![], vec![counted("a.txt")], false, cx);
    patches(&DiffSource::Unstaged, &[counted("a.txt")], "new", cx);
    let opened = open(DiffSource::Unstaged, counted("a.txt"), cx);
    opened
        .whole
        .respond(|_, _| Task::ready(Err(DataError::msg("The index is locked."))));

    opened.click("stage-file", cx);
    assert_eq!(
        opened.staging_error(cx).as_deref(),
        Some("The index is locked.")
    );
    assert_eq!(opened.events(), []);
}

#[gpui_kit::test]
fn opens_the_next_file_once_its_staged_without_waiting_for_the_lists(cx: &mut TestAppContext) {
    let _dir = init(cx);
    let unstaged = vec![counted("a.txt"), counted("b.txt")];
    set_changes(vec![], unstaged.clone(), false, cx);
    patches(&DiffSource::Unstaged, &unstaged, "new", cx);
    let opened = open(DiffSource::Unstaged, counted("a.txt"), cx);
    // The lists aren't reloaded: the status stays as it was.
    opened.whole.respond(staged_whole());

    opened.click("stage-file", cx);
    assert_eq!(
        opened.events(),
        [opened_with(DiffSource::Unstaged, counted("b.txt"))]
    );
}

#[gpui_kit::test]
fn stages_nothing_else_while_its_staged_not_again_nor_its_lines(cx: &mut TestAppContext) {
    let _dir = init(cx);
    set_changes(vec![], vec![counted("a.txt")], false, cx);
    patches(&DiffSource::Unstaged, &[counted("a.txt")], "new", cx);
    let opened = open(DiffSource::Unstaged, counted("a.txt"), cx);
    let (done, finished) = oneshot::channel::<()>();
    let finished = Rc::new(RefCell::new(Some(finished)));
    opened.whole.respond(move |_, cx| {
        let finished = finished.borrow_mut().take();
        cx.background_spawn(async move {
            if let Some(finished) = finished {
                let _ = finished.await;
            }
            Ok(())
        })
    });

    opened.click("stage-file", cx);
    assert!(!opened.is_enabled("stage-file", cx));
    assert!(opened.staging(cx).unwrap().busy);
    opened.click("stage-file", cx);
    assert_eq!(opened.whole.calls().len(), 1);
    done.send(()).unwrap();
    cx.run_until_parked();
    assert!(!opened.staging(cx).unwrap().busy);
}

#[gpui_kit::test]
fn cant_be_staged_while_the_last_file_is_still_on_show_as_anothers_changes_load(
    cx: &mut TestAppContext,
) {
    let _dir = init(cx);
    let unstaged = vec![counted("a.txt"), counted("b.txt")];
    set_changes(vec![], unstaged, false, cx);
    patches(&DiffSource::Unstaged, &[counted("a.txt")], "new", cx);
    let _next = pending_patch(&DiffSource::Unstaged, &counted("b.txt"), cx);
    let opened = open(DiffSource::Unstaged, counted("a.txt"), cx);
    assert!(opened.shown(cx).is_some());

    opened.set_file(DiffSource::Unstaged, counted("b.txt"), cx);
    assert_eq!(opened.header(cx), "a.txt");
    assert!(!opened.is_enabled("stage-file", cx));
}

#[gpui_kit::test]
fn isnt_offered_for_a_repository_inside_this_one(cx: &mut TestAppContext) {
    let _dir = init(cx);
    let nested = with_status(file("sub/", None, None), FileStatus::Untracked);
    set_changes(vec![], vec![nested.clone()], false, cx);
    let opened = open(DiffSource::Unstaged, nested, cx);

    assert_eq!(opened.body(cx), Body::Nested);
    assert!(!opened.has("stage-file", cx));
}

#[gpui_kit::test]
fn isnt_offered_for_a_commits_file(cx: &mut TestAppContext) {
    let _dir = init(cx);
    commit_files(cx);
    let opened = open(commit(), counted("a.txt"), cx);

    assert!(opened.shown(cx).is_some());
    assert!(!opened.has("stage-file", cx));
}
