//! Placing a new worktree for a branch. Ported from
//! `components/repository/add-worktree-dialog.tsx`.

use std::sync::Arc;

use gitto_git::features::refs::schema::Ref;
use gitto_git::features::worktree::schema::Worktree;
use gitto_git::input::RepositoryId;
use gitto_repository::Repository;
use gitto_ui::git::status::branch_name;
use gitto_ui::git::worktree::{checked_out_in, suggest_worktree_folder};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{Disableable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Context, Entity, Focusable as _, FontWeight, SharedString, Subscription, Window, div, rems,
};

use super::branch_form::NAME_PLACEHOLDER;
use crate::components::ui::{FormField, Icon, IconName};
use crate::data::refs::refs_query;
use crate::data::repositories::{find, repositories_query};
use crate::data::worktree::{AddWorktreeInput, WorktreeActions, worktree_actions, worktrees_query};
use crate::data::{Operations, Query};
use crate::services::SystemContext;
use crate::state::lingering::Lingering;
use crate::theme::PaletteExt as _;

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
    repository_id: RepositoryId,
    repositories: Query<Vec<Repository>>,
    worktrees: Query<Arc<[Worktree]>>,
    refs: Query<Arc<[Ref]>>,
    actions: WorktreeActions,
    folder: Entity<InputState>,
    new_branch: Entity<InputState>,
    /// The branch it places a worktree for: still named while the dialog closes.
    branch: Lingering<String>,
    /// The folder last suggested, which a folder that was typed instead is kept over.
    suggested: String,
    /// The new branch's placeholder, as it was last set: it names the branch kept, unless one
    /// has to be named.
    placeholder: SharedString,
    open: bool,
    _subscriptions: Vec<Subscription>,
}

impl AddWorktreeDialog {
    pub fn new(repository_id: RepositoryId, window: &mut Window, cx: &mut Context<Self>) -> Self {
        AddWorktreeDialog::with_actions(repository_id, worktree_actions(), window, cx)
    }

    /// With `actions` adding the worktree: a test's.
    pub(crate) fn with_actions(
        repository_id: RepositoryId,
        actions: WorktreeActions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let folder = cx.new(|cx| InputState::new(window, cx).placeholder("/path/to/folder"));
        let new_branch = cx.new(|cx| InputState::new(window, cx));
        let on_input = |this: &mut Self,
                        _: &Entity<InputState>,
                        event: &InputEvent,
                        window: &mut Window,
                        cx: &mut Context<Self>| match event {
            InputEvent::PressEnter { .. } => this.submit(window, cx),
            InputEvent::Change => cx.notify(),
            _ => {}
        };
        let subscriptions = vec![
            cx.observe_global::<Operations>(|_, cx| cx.notify()),
            cx.subscribe_in(&folder, window, on_input),
            cx.subscribe_in(&new_branch, window, on_input),
        ];
        AddWorktreeDialog {
            repositories: repositories_query(cx),
            worktrees: worktrees_query(&repository_id, cx),
            refs: refs_query(&repository_id, cx),
            repository_id,
            actions,
            folder,
            new_branch,
            branch: Lingering::default(),
            suggested: String::new(),
            placeholder: SharedString::default(),
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
        if self.repository_id == repository_id {
            return;
        }
        self.worktrees = worktrees_query(&repository_id, cx);
        self.refs = refs_query(&repository_id, cx);
        self.repository_id = repository_id;
        for input in [&self.folder, &self.new_branch] {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        if self.open {
            self.open = false;
            window.close_dialog(cx);
        }
        cx.notify();
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
    fn clash(&self) -> Option<Worktree> {
        let worktrees = self.worktrees.data();
        let refs = self.refs.data();
        let branch = self.branch.get().map_or("", String::as_str);
        checked_out_in(worktrees.as_deref(), refs.as_deref(), branch).cloned()
    }

    /// Whether a new branch has to be named.
    pub fn needs_new_branch(&self) -> bool {
        self.clash().is_some()
    }

    /// What adding it does.
    pub fn summary(&self) -> String {
        let name = self.name();
        if let Some(clash) = self.clash() {
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
            && (!self.needs_new_branch() || !Self::typed(&self.new_branch, cx).is_empty())
    }

    /// Opens it for `branch`, by its full ref name, suggesting a folder afresh, unless one was
    /// typed and kept after a failure.
    pub fn open(&mut self, branch: String, window: &mut Window, cx: &mut Context<Self>) {
        // Out of the way of the folder and name, which are kept after a failure to fix them.
        self.actions.add.dismiss(&self.repository_id, cx);
        let folder = Self::typed(&self.folder, cx);
        let repository = self
            .repositories
            .data()
            .and_then(|list| find(&list, &self.repository_id).cloned());
        if (folder.is_empty() || folder == self.suggested)
            && let Some(repository) = repository
        {
            self.suggested = suggest_worktree_folder(&repository.path, branch_name(&branch));
            let suggested = self.suggested.clone();
            self.folder
                .update(cx, |input, cx| input.set_value(suggested, window, cx));
        }
        self.branch.update(Some(branch));
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
            dialog
                .w(rems(WIDTH).to_pixels(window.rem_size()))
                .close_button(false)
                .on_cancel(move |_, _, cx| {
                    let _ = cancel.update(cx, |this, cx| {
                        this.open = false;
                        cx.notify();
                    });
                    true
                })
                .child(Self::render_form(&shown, window, cx))
        });
        focus.focus(window, cx);
        cx.notify();
    }

    fn render_form(this: &Entity<Self>, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        let (summary, needs_new_branch, name, folder, new_branch, valid, blocked) = {
            let shown = this.read(cx);
            (
                shown.summary(),
                shown.needs_new_branch(),
                shown.name(),
                shown.folder.clone(),
                shown.new_branch.clone(),
                shown.is_valid(cx),
                WorktreeActions::add_is_blocked(&shown.repository_id, cx),
            )
        };
        let placeholder: SharedString = if needs_new_branch {
            NAME_PLACEHOLDER.into()
        } else {
            format!("Keep {name}").into()
        };
        if this.read(cx).placeholder != placeholder {
            this.update(cx, |this, cx| {
                this.placeholder = placeholder.clone();
                this.new_branch.update(cx, |input, cx| {
                    input.set_placeholder(placeholder, window, cx)
                });
            });
        }
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
            .child(FormField::input("Folder", &folder).hint("Empty, or not there yet"))
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
                        Button::new("browse")
                            .label("Browse…")
                            .on_click(move |_, window, cx| {
                                let _ = browse.update(cx, |this, cx| this.browse(window, cx));
                            }),
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
                    let picked = picked.to_string_lossy().into_owned();
                    this.folder
                        .update(cx, |input, cx| input.set_value(picked, window, cx));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Adds the worktree, and closes the dialog. Once it's added, the folder and name are
    /// cleared, unless others are typed by then.
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open
            || !self.is_valid(cx)
            || WorktreeActions::add_is_blocked(&self.repository_id, cx)
        {
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
        let input = AddWorktreeInput {
            path: path.clone(),
            branch,
            new_branch: (!made.is_empty()).then(|| made.clone()),
        };
        let Some(done) = self.actions.add.run(&self.repository_id, input, cx) else {
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

    use gitto_git::features::refs::schema::RefKind;
    use gpui_kit::{AnyWindowHandle, AppContext as _, IntoElement, Render, TestAppContext};

    use super::*;
    use crate::components::repository::test_support::{
        Repositories, repository, repository_id, set_refs, set_worktrees, setup_with,
    };
    use crate::data::{QueryError, RepositoryOperation};

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
        adds: &Rc<RefCell<Vec<AddWorktreeInput>>>,
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
        let busy = |id: &str, cx: &App| WorktreeActions::is_pending(id, cx);
        let unused = RepositoryOperation::new("unused", |_, _: String, cx: &mut App| {
            cx.background_spawn(async { Ok::<_, QueryError>(()) })
        });
        let actions = WorktreeActions {
            add: RepositoryOperation::new("add-worktree", {
                let adds = Rc::clone(adds);
                move |_, input: AddWorktreeInput, cx: &mut App| {
                    adds.borrow_mut().push(input);
                    cx.background_spawn(async { Ok(()) })
                }
            })
            .one_at_a_time(WorktreeActions::add_is_blocked),
            remove: unused.clone().one_at_a_time(busy),
            open: unused.one_at_a_time(busy),
        };
        let (window, host) = crate::test_support::open(cx, move |window, cx| Host {
            dialog: cx.new(|cx| AddWorktreeDialog::with_actions(id, actions, window, cx)),
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
        let adds = Rc::new(RefCell::new(Vec::new()));
        let (_dir, window, dialog) = render(cx, &adds);
        open(window, &dialog, "refs/heads/topic", cx);

        let folder = dialog.read_with(cx, |dialog, _| dialog.folder.clone());
        assert_eq!(value(&folder, cx), "/work/gitto-topic");
        assert_eq!(
            dialog.read_with(cx, |dialog, _| dialog.summary()),
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
            *adds.borrow(),
            [AddWorktreeInput {
                path: "/work/gitto-topic".into(),
                branch: "refs/heads/topic".into(),
                new_branch: None,
            }]
        );
        assert!(!dialog.read_with(cx, |dialog, _| dialog.is_open()));
        assert_eq!(value(&folder, cx), "");
    }

    #[gpui_kit::test]
    fn needs_a_new_branch_for_one_checked_out_already(cx: &mut TestAppContext) {
        let adds = Rc::new(RefCell::new(Vec::new()));
        let (_dir, window, dialog) = render(cx, &adds);
        open(window, &dialog, "refs/remotes/origin/main", cx);

        assert!(dialog.read_with(cx, |dialog, _| dialog.needs_new_branch()));
        assert_eq!(
            dialog.read_with(cx, |dialog, _| dialog.summary()),
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
    fn keeps_a_folder_that_was_typed_over_the_one_suggested(cx: &mut TestAppContext) {
        let adds = Rc::new(RefCell::new(Vec::new()));
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
