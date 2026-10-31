//! Placing a new worktree for a branch. Ported from
//! `components/repository/add-worktree-dialog.tsx`.

use gitto_git::features::worktree::schema::Worktree;
use gitto_git::input::RepositoryId;
use gitto_ui_logic::git::status::branch_name;
use gitto_ui_logic::git::worktree::{checked_out_in, suggest_worktree_folder};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{Disableable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Context, Entity, FocusHandle, Focusable as _, FontWeight, SharedString, Subscription,
    Window, div, rems,
};

use super::branch_form::NAME_PLACEHOLDER;
use gitto_app_data::data::Change;
use gitto_app_data::data::repositories::find;
use gitto_app_data::data::worktree::AddWorktreeInput;
use gitto_app_data::data::{AppData, AppPart, Part, Repository, Shown};
use gitto_app_data::services::SystemContext;
use gitto_app_data::state::lingering::Lingering;
use gitto_app_kit::theme::PaletteExt as _;
use gitto_app_kit::ui::{FormField, Icon, IconName};

/// The folder's hint once one was picked whose path isn't UTF-8.
const UNUSABLE_PICK: &str = "That folder's path can't be used: pick or type another";

/// How wide the dialog is (`max-w-[400px]`), in rems of the 15px base.
const WIDTH: f32 = 26.67;

/// Places a new worktree for a branch, by its full ref name, and adds it. The folder is suggested
/// beside the repository, named after it and the branch, and can be typed or picked. The branch is
/// checked out there (for a remote one, the local branch tracking it, or a new one that does),
/// unless that's checked out in a worktree already (here, say): then a new branch, which has to be
/// named, is made from it and checked out there instead; one can be named either way. The
/// worktrees' section in the sidebar shows it running, and why it failed; the folder and name are
/// kept to fix. An entity, kept by the branch menus' provider, of one repository: another starts
/// with neither typed.
pub struct AddWorktreeDialog {
    repository: Shown,
    folder: Entity<InputState>,
    new_branch: Entity<InputState>,
    /// The branch it places a worktree for: still named while the dialog closes.
    branch: Lingering<String>,
    /// The folder last suggested, which a folder that was typed instead is kept over.
    suggested: String,
    /// The new branch's placeholder, as it was last set: it names the branch kept, unless one
    /// has to be named.
    placeholder: SharedString,
    /// The folder last picked had a path that isn't UTF-8, which git is given as text: it's
    /// said so, until another is picked or typed.
    unusable_pick: bool,
    /// Around Browse…, to tell whether Enter is on it.
    browse_focus: FocusHandle,
    open: bool,
    _subscriptions: Vec<Subscription>,
}

impl AddWorktreeDialog {
    pub fn new(repository_id: RepositoryId, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let folder = cx.new(|cx| InputState::new(window, cx).placeholder("/path/to/folder"));
        let new_branch = cx.new(|cx| InputState::new(window, cx));
        // Enter in them is the dialog's: see its `on_ok`.
        let on_input = |this: &mut Self,
                        input: Entity<InputState>,
                        event: &InputEvent,
                        cx: &mut Context<Self>| {
            if let InputEvent::Change = event {
                if input == this.folder {
                    this.unusable_pick = false;
                }
                cx.notify()
            }
        };
        let subscriptions = vec![
            cx.subscribe(&folder, on_input),
            cx.subscribe(&new_branch, on_input),
            // The worktrees and refs, which say whether a new branch has to be named, notify it.
            cx.observe_in(&cx.entity(), window, |this, _, window, cx| {
                this.update_placeholder(window, cx)
            }),
        ];
        AppData::global(cx).update(cx, |data, cx| data.show(AppPart::Repositories, cx));
        AddWorktreeDialog {
            repository: Shown::new(
                &repository_id,
                &[Part::Worktrees, Part::Refs, Part::Remotes, Part::Changes],
                cx,
            ),
            folder,
            new_branch,
            branch: Lingering::default(),
            suggested: String::new(),
            placeholder: SharedString::default(),
            unusable_pick: false,
            browse_focus: cx.focus_handle(),
            open: false,
            _subscriptions: subscriptions,
        }
    }

    /// Of another repository: a folder and a name typed in one aren't for another, and the dialog
    /// closes.
    pub fn set_repository(
        &mut self,
        repository_id: RepositoryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.repository.set(&repository_id, cx) {
            return;
        }
        for input in [&self.folder, &self.new_branch] {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        self.unusable_pick = false;
        if self.open {
            self.open = false;
            window.close_dialog(cx);
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn folder(&self) -> &Entity<InputState> {
        &self.folder
    }

    pub fn new_branch(&self) -> &Entity<InputState> {
        &self.new_branch
    }

    /// The branch's short name.
    fn name(&self) -> String {
        branch_name(self.branch.get().map_or("", String::as_str)).to_owned()
    }

    /// Where the branch (for a remote one, the local branch tracking it) is checked out already,
    /// if anywhere: then a new branch has to be made.
    fn clash(&self, cx: &App) -> Option<Worktree> {
        let repository = self.repository.read(cx);
        let worktrees = repository.worktrees().data()?;
        let refs = repository.refs().data().map_or(&[][..], |refs| &refs[..]);
        let remotes = repository.remotes().data().map_or(&[][..], Vec::as_slice);
        let branch = self.branch.get().map_or("", String::as_str);
        checked_out_in(worktrees, refs, remotes, branch).cloned()
    }

    /// Whether a new branch has to be named.
    pub fn needs_new_branch(&self, cx: &App) -> bool {
        self.clash(cx).is_some()
    }

    /// What adding it does.
    pub fn summary(&self, cx: &App) -> String {
        let name = self.name();
        if let Some(clash) = self.clash(cx) {
            let here = if clash.current {
                "here".to_owned()
            } else {
                format!("in {}", clash.name)
            };
            // For a remote branch, it's the local branch tracking it that is.
            let local = clash.branch.as_deref().map(branch_name);
            let which = match local {
                Some(local) if local != name => format!("{local}, which tracks {name},"),
                _ => name,
            };
            return format!(
                "{which} is checked out {here}, so a new branch is made from it and checked out \
                 in a folder of its own."
            );
        }
        let what = if self
            .branch
            .get()
            .is_some_and(|branch| branch.starts_with("refs/remotes/"))
        {
            format!("the local branch tracking {name}, or a new one that does,")
        } else {
            name
        };
        format!(
            "Checks out {what} in a folder of its own. Name a new branch to make one from it \
             there instead."
        )
    }

    fn typed(input: &Entity<InputState>, cx: &App) -> String {
        input.read(cx).value().trim().to_owned()
    }

    /// Whether what's typed can be added: a folder, and a new branch if one has to be named.
    pub fn is_valid(&self, cx: &App) -> bool {
        !Self::typed(&self.folder, cx).is_empty()
            && (!self.needs_new_branch(cx) || !Self::typed(&self.new_branch, cx).is_empty())
    }

    /// Opens it for `branch`, by its full ref name, suggesting a folder afresh, unless one was
    /// typed and kept after a failure.
    pub fn open(&mut self, branch: String, window: &mut Window, cx: &mut Context<Self>) {
        // Out of the way of the folder and name, which are kept after a failure to fix them.
        self.repository.update(cx, |repository, cx| {
            repository.dismiss(Change::AddWorktree, cx)
        });
        self.unusable_pick = false;
        let folder = Self::typed(&self.folder, cx);
        let id = self.repository.id(cx);
        let repository = AppData::global(cx)
            .read(cx)
            .repositories()
            .data()
            .and_then(|list| find(list, id).cloned());
        if (folder.is_empty() || folder == self.suggested)
            && let Some(repository) = repository
        {
            self.suggested = suggest_worktree_folder(&repository.path, branch_name(&branch));
            let suggested = self.suggested.clone();
            self.folder
                .update(cx, |input, cx| input.set_value(suggested, window, cx));
        }
        self.branch.update(Some(branch));
        self.update_placeholder(window, cx);
        if self.open {
            cx.notify();
            return;
        }
        self.open = true;
        let this = cx.entity().downgrade();
        let focus = self.folder.focus_handle(cx);
        window.open_dialog(cx, move |dialog, window, cx| {
            let Some(shown) = this.upgrade() else {
                return dialog;
            };
            let cancel = this.clone();
            let ok = this.clone();
            dialog
                .w(rems(WIDTH).to_pixels(window.rem_size()))
                .close_button(false)
                // Enter: on Browse…, it browses; elsewhere it submits, and closes itself, if it
                // can, and stays open otherwise.
                .on_ok(move |_, window, cx| {
                    let _ = ok.update(cx, |this, cx| {
                        if this.browse_focus.contains_focused(window, cx) {
                            this.browse(window, cx);
                        } else {
                            this.submit(window, cx);
                        }
                    });
                    false
                })
                .on_cancel(move |_, _, cx| {
                    let _ = cancel.update(cx, |this, cx| {
                        this.open = false;
                        cx.notify();
                    });
                    true
                })
                .child(Self::render_form(&shown, cx))
        });
        focus.focus(window, cx);
        cx.notify();
    }

    /// Names the branch kept in the new branch's placeholder, unless one has to be named.
    fn update_placeholder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let placeholder: SharedString = if self.needs_new_branch(cx) {
            NAME_PLACEHOLDER.into()
        } else {
            format!("Keep {}", self.name()).into()
        };
        if self.placeholder != placeholder {
            self.placeholder = placeholder.clone();
            self.new_branch.update(cx, |input, cx| {
                input.set_placeholder(placeholder, window, cx)
            });
        }
    }

    fn render_form(this: &Entity<Self>, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        let (summary, needs_new_branch, folder, new_branch, valid, blocked, unusable_pick) = {
            let shown = this.read(cx);
            (
                shown.summary(cx),
                shown.needs_new_branch(cx),
                shown.folder.clone(),
                shown.new_branch.clone(),
                shown.is_valid(cx),
                shown.repository.read(cx).add_worktree_blocked(),
                shown.unusable_pick,
            )
        };
        let browse_focus = this.read(cx).browse_focus.clone();
        let naming = !Self::typed(&new_branch, cx).is_empty();
        let browse = this.downgrade();
        let submit = this.downgrade();
        v_flex()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::BOLD)
                    .text_color(palette.text)
                    .child("New worktree"),
            )
            .child(
                div()
                    .mt_1()
                    .mb_2p5()
                    .text_xs()
                    .text_color(palette.muted)
                    .child(summary),
            )
            .child(FormField::input("Folder", &folder).hint(if unusable_pick {
                UNUSABLE_PICK
            } else {
                "Empty, or not there yet"
            }))
            .child(FormField::input(
                if needs_new_branch {
                    "New branch"
                } else {
                    "New branch (optional)"
                },
                &new_branch,
            ))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div().track_focus(&browse_focus).child(
                            Button::new("browse").label("Browse…").on_click(
                                move |_, window, cx| {
                                    let _ = browse.update(cx, |this, cx| this.browse(window, cx));
                                },
                            ),
                        ),
                    )
                    .child(
                        Button::new("add-worktree")
                            .primary()
                            .flex_1()
                            .icon(Icon::new(IconName::FolderPlus))
                            .label(if naming {
                                "Add worktree with new branch"
                            } else {
                                "Add worktree"
                            })
                            .disabled(!valid || blocked)
                            .on_click(move |_, window, cx| {
                                let _ = submit.update(cx, |this, cx| this.submit(window, cx));
                            }),
                    ),
            )
    }

    /// Picks the folder in the system's folder picker.
    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picking = SystemContext::select_folder(cx);
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Some(picked)) = picking.await {
                let _ = this.update_in(cx, |this, window, cx| {
                    // Not made into another path, as a lossy conversion would.
                    match picked.into_os_string().into_string() {
                        Ok(picked) => {
                            this.unusable_pick = false;
                            this.folder
                                .update(cx, |input, cx| input.set_value(picked, window, cx));
                        }
                        Err(_) => this.unusable_pick = true,
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Adds the worktree, and closes the dialog. Once it's added, the folder and name are
    /// cleared, unless others are typed by then.
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open || !self.is_valid(cx) || self.repository.read(cx).add_worktree_blocked() {
            return;
        }
        let Some(branch) = self.branch.get().cloned() else {
            return;
        };
        let path = Self::typed(&self.folder, cx);
        let made = Self::typed(&self.new_branch, cx);
        self.open = false;
        window.close_dialog(cx);
        cx.notify();
        let new_branch = (!made.is_empty()).then(|| made.clone());
        let input = AddWorktreeInput::parse(path.clone(), branch, new_branch);
        let done = self.repository.update(cx, |repository, cx| {
            let add = Repository::add_worktree;
            repository.parsing(Change::AddWorktree, input, add, cx)
        });
        let Some(done) = done else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(())) = done.await {
                let _ = this.update_in(cx, |this, window, cx| {
                    // Only what was used: something else may be typed in another repository by
                    // then.
                    if Self::typed(&this.folder, cx) == path {
                        this.folder
                            .update(cx, |input, cx| input.set_value("", window, cx));
                    }
                    if Self::typed(&this.new_branch, cx) == made {
                        this.new_branch
                            .update(cx, |input, cx| input.set_value("", window, cx));
                    }
                });
            }
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use futures::FutureExt as _;
    use gitto_git::features::refs::schema::{Ref, RefKind};
    use gitto_git::input::{BranchName, BranchRef, Folder};
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AnyWindowHandle, AppContext as _, IntoElement, Render, TestAppContext};

    use super::*;
    use crate::components::repository::test_support::{
        Repositories, click_label, fake_git, has_dialog, repository, repository_id, set_refs,
        set_remotes, set_worktrees, setup_with,
    };
    use gitto_app_data::data::git::{GitTask, RepositoryGit};

    /// A repository's git that records the worktrees asked to be added.
    #[derive(Default)]
    struct Adds(RefCell<Vec<AddWorktreeInput>>);

    impl RepositoryGit for Adds {
        fn add_worktree(
            &self,
            folder: Folder,
            branch: BranchRef,
            new_branch: Option<BranchName>,
        ) -> GitTask<()> {
            self.0.borrow_mut().push(AddWorktreeInput {
                path: folder,
                branch,
                new_branch,
            });
            async { Ok(()) }.boxed()
        }
    }

    struct Host {
        dialog: Entity<AddWorktreeDialog>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    fn worktree(path: &str, branch: Option<&str>, current: bool) -> Worktree {
        Worktree {
            path: path.into(),
            name: path.rsplit('/').next().unwrap().into(),
            head: Some("abc".into()),
            branch: branch.map(Into::into),
            main: current,
            bare: false,
            current,
            locked: None,
            prunable: None,
        }
    }

    fn local(name: &str, upstream: Option<&str>) -> Ref {
        Ref {
            name: name.into(),
            full_name: format!("refs/heads/{name}"),
            kind: RefKind::Local,
            sha: "abc".into(),
            current: false,
            upstream: upstream.map(Into::into),
            ahead: 0,
            behind: 0,
        }
    }

    fn render(
        cx: &mut TestAppContext,
        adds: &Rc<Adds>,
    ) -> (
        tempfile::TempDir,
        AnyWindowHandle,
        Entity<AddWorktreeDialog>,
    ) {
        let id = repository_id();
        let repositories = Rc::new(Repositories::default());
        repositories
            .list
            .borrow_mut()
            .push(repository(&id, "/work/gitto"));
        let dir = setup_with(cx, repositories);
        set_worktrees(
            &id,
            vec![worktree("/work/gitto", Some("refs/heads/main"), true)],
            cx,
        );
        set_refs(
            &id,
            vec![local("main", Some("origin/main")), local("topic", None)],
            cx,
        );
        set_remotes(&id, &["origin"], cx);
        fake_git(&id, adds.clone(), cx);
        let (window, host) = crate::test_support::open(cx, move |window, cx| Host {
            dialog: cx.new(|cx| AddWorktreeDialog::new(id, window, cx)),
        });
        cx.run_until_parked();
        let dialog = host.read_with(cx, |host, _| host.dialog.clone());
        (dir, window, dialog)
    }

    fn open(
        window: AnyWindowHandle,
        dialog: &Entity<AddWorktreeDialog>,
        branch: &str,
        cx: &mut TestAppContext,
    ) {
        cx.update_window(window, |_, window, cx| {
            dialog.update(cx, |dialog, cx| dialog.open(branch.into(), window, cx))
        })
        .unwrap();
        cx.run_until_parked();
    }

    fn value(input: &Entity<InputState>, cx: &mut TestAppContext) -> String {
        input.read_with(cx, |input, _| input.value().to_string())
    }

    #[gpui_kit::test]
    fn suggests_a_folder_beside_the_repository_and_checks_the_branch_out_there(
        cx: &mut TestAppContext,
    ) {
        let adds = Rc::new(Adds::default());
        let (_dir, window, dialog) = render(cx, &adds);
        open(window, &dialog, "refs/heads/topic", cx);

        let folder = dialog.read_with(cx, |dialog, _| dialog.folder.clone());
        assert_eq!(value(&folder, cx), "/work/gitto-topic");
        assert_eq!(
            dialog.read_with(cx, |dialog, cx| dialog.summary(cx)),
            "Checks out topic in a folder of its own. Name a new branch to make one from it \
             there instead."
        );
        assert!(dialog.read_with(cx, |dialog, cx| dialog.is_valid(cx)));

        cx.update_window(window, |_, window, cx| {
            dialog.update(cx, |dialog, cx| dialog.submit(window, cx))
        })
        .unwrap();
        cx.run_until_parked();
        assert_eq!(
            *adds.0.borrow(),
            [AddWorktreeInput::parse("/work/gitto-topic", "refs/heads/topic", None).unwrap()]
        );
        assert!(!dialog.read_with(cx, |dialog, _| dialog.is_open()));
        assert_eq!(value(&folder, cx), "");
    }

    fn press(window: AnyWindowHandle, keys: &[&str], cx: &mut TestAppContext) {
        cx.update_window(window, |_, window, cx| {
            for key in keys {
                window.press(key, cx);
            }
        })
        .unwrap();
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn stays_open_on_enter_without_a_folder(cx: &mut TestAppContext) {
        let adds = Rc::new(Adds::default());
        let (_dir, window, dialog) = render(cx, &adds);
        open(window, &dialog, "refs/heads/topic", cx);
        let folder = dialog.read_with(cx, |dialog, _| dialog.folder.clone());
        cx.update_window(window, |_, window, cx| {
            folder.update(cx, |input, cx| input.set_value("", window, cx))
        })
        .unwrap();

        press(window, &["enter"], cx);
        assert!(adds.0.borrow().is_empty());
        assert!(dialog.read_with(cx, |dialog, _| dialog.is_open()));
        assert!(has_dialog(window, cx));

        // Still told when it's closed, so it opens again after.
        press(window, &["escape"], cx);
        assert!(!dialog.read_with(cx, |dialog, _| dialog.is_open()));
        assert!(!has_dialog(window, cx));
    }

    #[gpui_kit::test]
    fn adds_it_on_enter_on_add_worktree(cx: &mut TestAppContext) {
        let adds = Rc::new(Adds::default());
        let (_dir, window, dialog) = render(cx, &adds);
        open(window, &dialog, "refs/heads/topic", cx);
        // From the folder, past the new branch and Browse….
        press(window, &["tab", "tab", "tab"], cx);
        let inputs = dialog.read_with(cx, |dialog, cx| {
            [&dialog.folder, &dialog.new_branch].map(|input| input.focus_handle(cx))
        });
        cx.update_window(window, |_, window, _| {
            assert!(!inputs.iter().any(|input| input.is_focused(window)))
        })
        .unwrap();
        press(window, &["enter"], cx);
        assert_eq!(adds.0.borrow().len(), 1);
        assert!(!cx.did_prompt_for_paths());
        assert!(!dialog.read_with(cx, |dialog, _| dialog.is_open()));
        assert!(!has_dialog(window, cx));
    }

    #[gpui_kit::test]
    fn browses_on_enter_on_browse(cx: &mut TestAppContext) {
        let adds = Rc::new(Adds::default());
        let (_dir, window, dialog) = render(cx, &adds);
        open(window, &dialog, "refs/heads/topic", cx);
        press(window, &["tab", "tab", "enter"], cx);
        assert!(cx.did_prompt_for_paths());
        assert!(adds.0.borrow().is_empty());
        assert!(dialog.read_with(cx, |dialog, _| dialog.is_open()));
    }

    #[gpui_kit::test]
    fn closes_only_itself_on_enter(cx: &mut TestAppContext) {
        let adds = Rc::new(Adds::default());
        let (_dir, window, dialog) = render(cx, &adds);
        cx.update_window(window, |_, window, cx| {
            window.open_dialog(cx, |dialog, _, _| dialog)
        })
        .unwrap();
        open(window, &dialog, "refs/heads/topic", cx);
        press(window, &["enter"], cx);
        assert_eq!(adds.0.borrow().len(), 1);
        assert!(!dialog.read_with(cx, |dialog, _| dialog.is_open()));
        // The one under it is still open.
        assert!(has_dialog(window, cx));
    }

    #[gpui_kit::test]
    fn needs_a_new_branch_for_one_checked_out_already(cx: &mut TestAppContext) {
        let adds = Rc::new(Adds::default());
        let (_dir, window, dialog) = render(cx, &adds);
        open(window, &dialog, "refs/remotes/origin/main", cx);

        assert!(dialog.read_with(cx, |dialog, cx| dialog.needs_new_branch(cx)));
        assert_eq!(
            dialog.read_with(cx, |dialog, cx| dialog.summary(cx)),
            "main, which tracks origin/main, is checked out here, so a new branch is made from \
             it and checked out in a folder of its own."
        );
        // A folder alone isn't enough.
        assert!(!dialog.read_with(cx, |dialog, cx| dialog.is_valid(cx)));
        let new_branch = dialog.read_with(cx, |dialog, _| dialog.new_branch.clone());
        cx.update_window(window, |_, window, cx| {
            new_branch.update(cx, |input, cx| input.set_value("hotfix", window, cx))
        })
        .unwrap();
        assert!(dialog.read_with(cx, |dialog, cx| dialog.is_valid(cx)));
    }

    #[gpui_kit::test]
    fn names_the_branch_kept_in_the_placeholder_until_one_has_to_be_named(cx: &mut TestAppContext) {
        let adds = Rc::new(Adds::default());
        let (_dir, window, dialog) = render(cx, &adds);
        let placeholder = |cx: &mut TestAppContext| {
            dialog.read_with(cx, |dialog, _| dialog.placeholder.to_string())
        };
        open(window, &dialog, "refs/heads/topic", cx);
        assert_eq!(placeholder(cx), "Keep topic");

        // Checked out elsewhere meanwhile: one has to be named.
        let id = dialog.read_with(cx, |dialog, cx| dialog.repository.id(cx).clone());
        set_worktrees(
            &id,
            vec![
                worktree("/work/gitto", Some("refs/heads/main"), true),
                worktree("/work/gitto-topic", Some("refs/heads/topic"), false),
            ],
            cx,
        );
        cx.run_until_parked();
        assert_eq!(placeholder(cx), NAME_PLACEHOLDER);
    }

    #[cfg(unix)]
    #[gpui_kit::test]
    fn refuses_a_picked_folder_whose_path_isnt_utf8(cx: &mut TestAppContext) {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt as _;
        use std::path::PathBuf;

        let adds = Rc::new(Adds::default());
        let (_dir, window, dialog) = render(cx, &adds);
        open(window, &dialog, "refs/heads/topic", cx);
        let folder = dialog.read_with(cx, |dialog, _| dialog.folder.clone());
        let pick = |path: PathBuf, cx: &mut TestAppContext| {
            click_label(window, "Browse…", cx);
            assert!(cx.did_prompt_for_paths());
            cx.simulate_path_prompt_response(move |_| Some(vec![path]));
            cx.run_until_parked();
        };

        pick(PathBuf::from(OsStr::from_bytes(b"/work/gitto-\xff")), cx);
        assert_eq!(value(&folder, cx), "/work/gitto-topic");
        assert!(dialog.read_with(cx, |dialog, _| dialog.unusable_pick));

        pick(PathBuf::from("/work/elsewhere"), cx);
        assert_eq!(value(&folder, cx), "/work/elsewhere");
        assert!(!dialog.read_with(cx, |dialog, _| dialog.unusable_pick));
    }

    #[gpui_kit::test]
    fn closes_and_forgets_the_folder_and_name_for_another_repository(cx: &mut TestAppContext) {
        let adds = Rc::new(Adds::default());
        let (_dir, window, dialog) = render(cx, &adds);
        open(window, &dialog, "refs/remotes/origin/main", cx);
        let (folder, new_branch) = dialog.read_with(cx, |dialog, _| {
            (dialog.folder.clone(), dialog.new_branch.clone())
        });
        cx.update_window(window, |_, window, cx| {
            new_branch.update(cx, |input, cx| input.set_value("hotfix", window, cx))
        })
        .unwrap();
        assert_ne!(value(&folder, cx), "");

        let other = repository_id();
        set_worktrees(&other, Vec::new(), cx);
        set_refs(&other, Vec::new(), cx);
        set_remotes(&other, &[], cx);
        cx.update_window(window, |_, window, cx| {
            dialog.update(cx, |dialog, cx| dialog.set_repository(other, window, cx))
        })
        .unwrap();
        cx.run_until_parked();
        assert!(!dialog.read_with(cx, |dialog, _| dialog.is_open()));
        assert!(!has_dialog(window, cx));
        assert_eq!(value(&folder, cx), "");
        assert_eq!(value(&new_branch, cx), "");
        assert!(adds.0.borrow().is_empty());
    }

    #[gpui_kit::test]
    fn keeps_a_folder_that_was_typed_over_the_one_suggested(cx: &mut TestAppContext) {
        let adds = Rc::new(Adds::default());
        let (_dir, window, dialog) = render(cx, &adds);
        open(window, &dialog, "refs/heads/topic", cx);
        let folder = dialog.read_with(cx, |dialog, _| dialog.folder.clone());
        cx.update_window(window, |_, window, cx| {
            folder.update(cx, |input, cx| input.set_value("/elsewhere", window, cx));
            window.close_dialog(cx);
        })
        .unwrap();
        dialog.update(cx, |dialog, _| dialog.open = false);

        open(window, &dialog, "refs/heads/main", cx);
        assert_eq!(value(&folder, cx), "/elsewhere");
    }
}
