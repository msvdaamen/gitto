//! A list of changed files. Ported from `components/repository/details/changed-file-list.tsx`.

use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration;

use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::schema::FileStatus;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Context, ElementId, Entity, FontWeight, Rems, Role, SharedString, Task,
    TestSupportExt as _, Window, div,
};

use crate::components::ui::{
    ContextMenuExt as _, FileStatusBadge, Icon, IconName, LineStats, PopupMenu, VirtualList,
};
use crate::data::Opaque;
use crate::theme::{PaletteExt as _, base_px};

/// How long the pointer rests on a file before it's taken for one about to be opened.
pub const HOVER: Duration = Duration::from_millis(80);

/// Height of a file's row, and of it with the space below it.
pub const ROW_HEIGHT: Rems = base_px(42.);
pub const ROW_PITCH: Rems = base_px(44.);

type FileHandler = Rc<dyn Fn(&ChangedFile, &mut Window, &mut App)>;
type FileMenuBuilder =
    Rc<dyn Fn(PopupMenu, &ChangedFile, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>;

/// An action shown on each file when it's hovered, like staging it.
#[derive(Clone)]
pub struct FileAction {
    label: SharedString,
    icon: IconName,
    disabled: bool,
    run: FileHandler,
}

impl FileAction {
    pub fn new(
        label: impl Into<SharedString>,
        icon: IconName,
        run: impl Fn(&ChangedFile, &mut Window, &mut App) + 'static,
    ) -> Self {
        FileAction {
            label: label.into(),
            icon,
            disabled: false,
            run: Rc::new(run),
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// Where the pointer rests in a list: once it's rested on a file for [`HOVER`], the file's changes
/// are loaded ahead, as it's likely to be opened. One per list, kept by its owner: the timer is
/// the list's, so passing over a file on the way to another loads nothing.
#[derive(Default)]
pub struct HoverIntent {
    timer: Option<Task<()>>,
}

impl HoverIntent {
    /// Runs `then` once the pointer has rested for [`HOVER`], unless it leaves first.
    pub fn rest(&mut self, then: impl FnOnce(&mut App) + 'static, cx: &mut Context<Self>) {
        self.timer = Some(cx.spawn(async move |_, cx| {
            cx.background_executor().timer(HOVER).await;
            cx.update(then);
        }));
    }

    pub fn leave(&mut self) {
        self.timer = None;
    }
}

/// The files of a commit, a stash, or a side of the uncommitted changes, as rows: each with its
/// status, its name and folder, and its lines added and removed. Built in `render` from what its
/// owner shows, cheap to clone, and rendered a row at a time ([`row`](Self::row)), so only the
/// ones in view are: a commit or the working tree can have tens of thousands. In a list that
/// scrolls on its own, [`virtual_list`](Self::virtual_list); with what's around it, in its owner's
/// `gpui::list`.
#[derive(Clone)]
pub struct ChangedFileList {
    files: Opaque<Vec<ChangedFile>>,
    /// The files' lines weren't counted, so one without line counts isn't known to be binary.
    uncounted: bool,
    action: Option<FileAction>,
    /// Shows a file's changes when it's clicked.
    on_open: Option<FileHandler>,
    /// The path of the file whose changes are on show, if one's in the list.
    open_path: Option<String>,
    /// Loads a file's changes ahead, once the pointer rests on it: it's likely to be opened.
    on_prefetch: Option<(Entity<HoverIntent>, FileHandler)>,
    /// The conflicted files with no conflict markers left, which are badged as ready.
    marker_free: Option<Opaque<HashSet<String>>>,
    /// The path of the file shown as if hovered, e.g. while its menu is open.
    highlighted_path: Option<String>,
    /// A menu of what can be done with a file, opened by right-clicking it.
    menu: Option<FileMenuBuilder>,
}

impl ChangedFileList {
    pub fn new(files: Opaque<Vec<ChangedFile>>) -> Self {
        ChangedFileList {
            files,
            uncounted: false,
            action: None,
            on_open: None,
            open_path: None,
            on_prefetch: None,
            marker_free: None,
            highlighted_path: None,
            menu: None,
        }
    }

    pub fn uncounted(mut self, uncounted: bool) -> Self {
        self.uncounted = uncounted;
        self
    }

    pub fn action(mut self, action: FileAction) -> Self {
        self.action = Some(action);
        self
    }

    pub fn on_open(mut self, open: impl Fn(&ChangedFile, &mut Window, &mut App) + 'static) -> Self {
        self.on_open = Some(Rc::new(open));
        self
    }

    pub fn open_path(mut self, path: Option<String>) -> Self {
        self.open_path = path;
        self
    }

    /// Loads a file's changes ahead with `prefetch`, once the pointer has rested on it, as
    /// `hover` tells.
    pub fn on_prefetch(
        mut self,
        hover: &Entity<HoverIntent>,
        prefetch: impl Fn(&ChangedFile, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_prefetch = Some((hover.clone(), Rc::new(prefetch)));
        self
    }

    pub fn marker_free(mut self, marker_free: Opaque<HashSet<String>>) -> Self {
        self.marker_free = Some(marker_free);
        self
    }

    pub fn highlighted_path(mut self, path: Option<String>) -> Self {
        self.highlighted_path = path;
        self
    }

    /// The menu `build` makes for a file, opened by right-clicking it.
    pub fn menu(
        mut self,
        build: impl Fn(PopupMenu, &ChangedFile, &mut Window, &mut Context<PopupMenu>) -> PopupMenu
        + 'static,
    ) -> Self {
        self.menu = Some(Rc::new(build));
        self
    }

    pub fn files(&self) -> &Opaque<Vec<ChangedFile>> {
        &self.files
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// The rows, in a list `id` that scrolls on its own, only those in view rendered.
    pub fn virtual_list(self, id: impl Into<ElementId>) -> VirtualList {
        let count = self.files.len();
        VirtualList::new(id, count, ROW_PITCH, move |range, _, cx| {
            range.map(|ix| self.row(ix, cx)).collect()
        })
    }

    /// The row of the file at `ix`, [`ROW_HEIGHT`] high.
    pub fn row(&self, ix: usize, cx: &App) -> AnyElement {
        let Some(file) = self.files.get(ix) else {
            return div().into_any_element();
        };
        let palette = *cx.palette();
        let path = file.path.clone();
        let open = self.open_path.as_deref() == Some(path.as_str());
        let highlighted = self.highlighted_path.as_deref() == Some(path.as_str());
        let (name, folder) = name_and_folder(&path);
        let ready = self
            .marker_free
            .as_ref()
            .is_some_and(|free| free.contains(&path));

        let stats: AnyElement = match (file.additions, lines_label(file, self.uncounted)) {
            (Some(additions), _) => {
                LineStats::new(additions, file.deletions.unwrap_or(0)).into_any_element()
            }
            (None, label) => div()
                .text_color(palette.faint)
                .child(label.unwrap_or_default())
                .into_any_element(),
        };

        // A button when the file opens, so its contents (the status's tooltip, say) still take
        // the pointer; its action is next to it, rather than inside.
        let opener = h_flex()
            .id(SharedString::from(format!("file:{path}")))
            .h_full()
            .min_w_0()
            .flex_1()
            .gap(base_px(7.))
            .p(base_px(7.))
            .rounded_md()
            // Named as the badge's tooltip says it, for whoever can't see its color.
            .child(
                div()
                    .id(SharedString::from(format!("status:{path}")))
                    .aria_label(status_title(file.status, ready))
                    .child(
                        FileStatusBadge::new(
                            SharedString::from(format!("badge:{path}")),
                            file.status,
                        )
                        .ready(ready),
                    )
                    .test_support(),
            )
            .child(
                v_flex()
                    .min_w_0()
                    .flex_1()
                    .gap_0p5()
                    .child(
                        div()
                            .truncate()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette.text)
                            .child(name.to_owned()),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(palette.faint)
                            .child(folder.to_owned()),
                    ),
            )
            .child(h_flex().flex_none().text_xs().child(stats))
            .when_some(self.on_open.clone(), |this, on_open| {
                let file = file.clone();
                this.role(Role::Button)
                    .aria_label(SharedString::from(format!("Show changes in {path}")))
                    .aria_selected(open)
                    .on_click(move |_, window, cx| on_open(&file, window, cx))
            })
            .when_some(self.on_prefetch.clone(), |this, (hover, prefetch)| {
                let file = file.clone();
                this.on_hover(move |hovered, window, cx| {
                    let handle = window.window_handle();
                    if *hovered {
                        let (file, prefetch) = (file.clone(), Rc::clone(&prefetch));
                        hover.update(cx, |hover, cx| {
                            hover.rest(
                                move |cx| {
                                    let _ = handle
                                        .update(cx, |_, window, cx| prefetch(&file, window, cx));
                                },
                                cx,
                            )
                        });
                    } else {
                        hover.update(cx, |hover, _| hover.leave());
                    }
                })
            })
            .test_support();

        let action = self.action.clone().map(|action| {
            let file = file.clone();
            div()
                .flex_none()
                .mr(base_px(7.))
                .opacity(0.)
                .group_hover("file-row", |style| style.opacity(1.))
                .child(
                    Button::new(SharedString::from(format!("action:{path}")))
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(action.icon))
                        .accessibility_label(SharedString::from(format!("{} {path}", action.label)))
                        .tooltip(action.label.clone())
                        .disabled(action.disabled)
                        .on_click(move |_, window, cx| (action.run)(&file, window, cx)),
                )
        });

        let row = h_flex()
            .id(SharedString::from(format!("row:{path}")))
            .group("file-row")
            .h(ROW_HEIGHT)
            .w_full()
            .rounded(cx.theme().radius)
            .when(!open, |this| {
                this.hover(|style| style.bg(palette.panel_hover))
            })
            .when(highlighted && !open, |this| this.bg(palette.panel_hover))
            .when(open, |this| this.bg(palette.panel_active))
            .child(opener)
            .children(action);

        match self.menu.clone() {
            Some(build) => {
                let file = file.clone();
                row.context_menu(move |menu, window, cx| build(menu, &file, window, cx))
                    .into_any_element()
            }
            None => row.into_any_element(),
        }
    }
}

/// What's said in place of a file's line counts when it has none: an untracked file is new; else
/// it's binary, unless no file's lines were counted (`uncounted`), when it's not known. `None` when
/// it has them.
pub fn lines_label(file: &ChangedFile, uncounted: bool) -> Option<&'static str> {
    if file.additions.is_some() {
        return None;
    }
    Some(if file.status == FileStatus::Untracked {
        "new"
    } else if uncounted {
        ""
    } else {
        "binary"
    })
}

/// What a file's status badge says on hover (see `FileStatusBadge`): its status, or, for a
/// conflicted file with no conflict markers left (`ready`), that it's ready to mark resolved.
pub fn status_title(status: FileStatus, ready: bool) -> &'static str {
    if status == FileStatus::Conflicted && ready {
        "conflicted, but resolved: ready to mark resolved"
    } else {
        status.as_str()
    }
}

/// A file's name, and the folder it's in ("root" at the top), as the list shows them.
pub fn name_and_folder(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(slash) => (&path[slash + 1..], &path[..slash]),
        None => (path, "root"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_a_file_and_its_folder() {
        assert_eq!(name_and_folder("src/lib/a.rs"), ("a.rs", "src/lib"));
        assert_eq!(name_and_folder("a.rs"), ("a.rs", "root"));
        // A repository inside this one, as git lists it.
        assert_eq!(name_and_folder("vendor/lib/"), ("", "vendor/lib"));
    }
}

#[cfg(test)]
mod ui_tests {
    use std::cell::RefCell;
    use std::collections::HashSet;

    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AnyWindowHandle, AppContext as _, Render, TestAppContext};

    use super::*;
    use crate::components::repository::details::test_fakes::open;

    fn files() -> Vec<ChangedFile> {
        vec![
            ChangedFile {
                path: "image.png".into(),
                status: FileStatus::Modified,
                orig_path: None,
                additions: None,
                deletions: None,
            },
            ChangedFile {
                path: "new.txt".into(),
                status: FileStatus::Untracked,
                orig_path: None,
                additions: None,
                deletions: None,
            },
        ]
    }

    /// A conflicted file at `path`.
    fn conflicted(path: &str) -> ChangedFile {
        ChangedFile {
            path: path.into(),
            status: FileStatus::Conflicted,
            orig_path: None,
            additions: Some(1),
            deletions: Some(1),
        }
    }

    /// A list of files, recording the ones opened and loaded ahead, beside somewhere else for the
    /// pointer to go.
    struct Files {
        files: Vec<ChangedFile>,
        open_path: Option<String>,
        marker_free: HashSet<String>,
        hover: Entity<HoverIntent>,
        opened: Rc<RefCell<Vec<ChangedFile>>>,
        prefetched: Rc<RefCell<Vec<ChangedFile>>>,
    }

    impl Render for Files {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let opened = Rc::clone(&self.opened);
            let prefetched = Rc::clone(&self.prefetched);
            let list = ChangedFileList::new(Opaque::new(self.files.clone()))
                .open_path(self.open_path.clone())
                .marker_free(Opaque::new(self.marker_free.clone()))
                .on_open(move |file, _, _| opened.borrow_mut().push(file.clone()))
                .on_prefetch(&self.hover, move |file, _, _| {
                    prefetched.borrow_mut().push(file.clone())
                });
            v_flex()
                .size_full()
                .child(div().h_64().child(list.virtual_list("files")))
                .child(div().id("elsewhere").h_16().w_full().test_support())
        }
    }

    fn render_files(
        cx: &mut TestAppContext,
        files: Vec<ChangedFile>,
        open_path: Option<&str>,
        marker_free: &[&str],
    ) -> (tempfile::TempDir, AnyWindowHandle, Entity<Files>) {
        let open_path = open_path.map(str::to_owned);
        let marker_free = marker_free.iter().map(|path| (*path).to_owned()).collect();
        open(cx, move |_, cx| Files {
            files,
            open_path,
            marker_free,
            hover: cx.new(|_| HoverIntent::default()),
            opened: Rc::default(),
            prefetched: Rc::default(),
        })
    }

    // `changed-file-list.test.tsx`, "a file without line counts": what the row says in their
    // place, which the GPUI test window can't read as text, is `lines_label`'s.
    #[test]
    fn is_binary_or_new_when_its_untracked() {
        let files = files();
        assert_eq!(lines_label(&files[0], false), Some("binary"));
        assert_eq!(lines_label(&files[1], false), Some("new"));
        assert_eq!(lines_label(&conflicted("a.txt"), false), None);
    }

    #[test]
    fn isnt_taken_for_binary_when_no_files_lines_were_counted() {
        let files = files();
        assert_eq!(lines_label(&files[0], true), Some(""));
        assert_eq!(lines_label(&files[1], true), Some("new"));
    }

    #[gpui_kit::test]
    fn loads_a_files_changes_ahead_once_the_pointer_rests_on_it(cx: &mut TestAppContext) {
        let (_dir, window, view) = render_files(cx, files(), None, &[]);
        let hover = |id: &'static str, cx: &mut TestAppContext| {
            cx.update_window(window, |_, window, cx| window.hover(id, cx))
                .unwrap();
        };

        // Passed over on the way somewhere else: nothing's loaded.
        hover("file:image.png", cx);
        hover("elsewhere", cx);
        cx.executor().advance_clock(Duration::from_millis(120));
        cx.run_until_parked();
        assert!(view.read_with(cx, |view, _| view.prefetched.borrow().is_empty()));

        hover("file:image.png", cx);
        cx.executor()
            .advance_clock(HOVER - Duration::from_millis(1));
        cx.run_until_parked();
        assert!(view.read_with(cx, |view, _| view.prefetched.borrow().is_empty()));
        cx.executor().advance_clock(Duration::from_millis(1));
        cx.run_until_parked();
        let prefetched = view.read_with(cx, |view, _| view.prefetched.borrow().clone());
        assert_eq!(prefetched, [files()[0].clone()]);
    }

    #[gpui_kit::test]
    fn shows_its_changes_when_its_clicked_and_marks_the_one_on_show(cx: &mut TestAppContext) {
        let (_dir, window, view) = render_files(cx, files(), Some("new.txt"), &[]);
        cx.update_window(window, |_, window, cx| window.click("file:image.png", cx))
            .unwrap();
        let opened = view.read_with(cx, |view, _| view.opened.borrow().clone());
        assert_eq!(opened, [files()[0].clone()]);

        cx.update_window(window, |_, window, _| {
            let open = window.find("file:new.txt");
            assert_eq!(open.label(), Some("Show changes in new.txt"));
            assert_eq!(open.selected(), Some(true));
            assert_ne!(window.find("file:image.png").selected(), Some(true));
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn is_badged_as_ready_once_it_has_no_conflict_markers_left(cx: &mut TestAppContext) {
        let files = vec![conflicted("a.txt"), conflicted("b.txt")];
        let (_dir, window, _) = render_files(cx, files, None, &["b.txt"]);
        cx.update_window(window, |_, window, _| {
            assert_eq!(window.find("status:a.txt").label(), Some("conflicted"));
            assert_eq!(
                window.find("status:b.txt").label(),
                Some("conflicted, but resolved: ready to mark resolved")
            );
        })
        .unwrap();
    }
}
