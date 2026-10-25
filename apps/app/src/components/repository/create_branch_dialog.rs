//! Naming a branch to create from another. Ported from
//! `components/repository/create-branch-dialog.tsx`.

use gitto_git::input::RepositoryId;
use gitto_ui_logic::git::status::{branch_name, has_uncommitted_changes, is_checked_out};
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, Focusable as _, Subscription, Window, rems};

use super::branch_form::{BranchForm, NAME_PLACEHOLDER, submitted_name};
use gitto_app_data::data::branch::NewBranchFrom;
use gitto_app_data::data::changes::SWITCH;
use gitto_app_data::data::{Part, Shown};
use gitto_app_data::state::lingering::Lingering;

/// How wide the dialog is (`max-w-[320px]`), in rems of the 15px base.
const WIDTH: f32 = 21.33;

/// Names a branch to create from another, by its full ref name, and switches to it, taking the
/// uncommitted changes along. That's a switch of branches: the current branch, in the toolbar,
/// shows it running, and why it failed. It can't be created while another switch, or a merge, is
/// running. An entity, kept by the branch menus' provider: the name typed is kept after a failure
/// to fix it, whichever branch's menu it's opened from again, and is for the repository it was
/// typed in.
pub struct CreateBranchDialog {
    repository: Shown,
    name: Entity<InputState>,
    /// The branch it's made from, by its full ref name: still named while the dialog closes.
    from: Lingering<String>,
    open: bool,
    _subscriptions: Vec<Subscription>,
}

impl CreateBranchDialog {
    pub fn new(repository_id: RepositoryId, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder(NAME_PLACEHOLDER));
        let subscriptions = vec![
            cx.subscribe_in(&name, window, |this, _, event, window, cx| match event {
                InputEvent::PressEnter { .. } => this.submit(window, cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            }),
        ];
        CreateBranchDialog {
            repository: Shown::new(&repository_id, &[Part::Status, Part::Operations], cx),
            name,
            from: Lingering::default(),
            open: false,
            _subscriptions: subscriptions,
        }
    }

    /// Of another repository: a name typed in one isn't for another, and the dialog closes.
    pub fn set_repository(
        &mut self,
        repository_id: RepositoryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.repository.set(&repository_id, cx) {
            return;
        }
        self.name
            .update(cx, |name, cx| name.set_value("", window, cx));
        self.close(window, cx);
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn name(&self) -> &Entity<InputState> {
        &self.name
    }

    /// Opens it to name a branch made from `from`, by its full ref name. Why the last one failed
    /// is put out of the way of the name, which is kept after a failure to fix it.
    pub fn open(&mut self, from: String, window: &mut Window, cx: &mut Context<Self>) {
        self.repository
            .update(cx, |repository, cx| repository.dismiss(SWITCH, cx));
        self.from.update(Some(from));
        if self.open {
            cx.notify();
            return;
        }
        self.open = true;
        let this = cx.entity();
        let focus = self.name.focus_handle(cx);
        window.open_dialog(cx, move |dialog, window, cx| {
            let (summary, name, disabled) = {
                let shown = this.read(cx);
                (
                    shown.summary(cx),
                    shown.name.clone(),
                    shown.repository.read(cx).branches_changing(),
                )
            };
            let cancel = this.clone();
            let submit = this.clone();
            dialog
                .w(rems(WIDTH).to_pixels(window.rem_size()))
                .close_button(false)
                .on_cancel(move |_, _, cx| {
                    cancel.update(cx, |this, cx| {
                        this.open = false;
                        cx.notify();
                    });
                    true
                })
                .child(
                    BranchForm::new(summary, &name)
                        // `run` does nothing while a switch or a merge is running.
                        .disabled(disabled)
                        .on_submit(move |_, window, cx| {
                            submit.update(cx, |this, cx| this.submit(window, cx));
                        }),
                )
        });
        focus.focus(window, cx);
        cx.notify();
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            self.open = false;
            window.close_dialog(cx);
            cx.notify();
        }
    }

    /// What the new branch is made from, and what comes along: from another commit than HEAD,
    /// the uncommitted changes can conflict with it.
    pub fn summary(&self, cx: &App) -> String {
        let from = self.from.get().map_or("", String::as_str);
        let status = self
            .repository
            .read(cx)
            .status()
            .data()
            .map(|status| status.summary());
        let changes = if !has_uncommitted_changes(status.as_ref()) {
            "."
        } else if status
            .as_ref()
            .is_some_and(|status| is_checked_out(&status.head, from))
        {
            ", with your uncommitted changes."
        } else {
            ", with your uncommitted changes, unless they conflict with it: then they're kept in \
             the stash."
        };
        format!("From {}{changes}", branch_name(from))
    }

    /// Creates the branch named, from the branch the dialog's for, and closes it. Once it's
    /// created, the name's cleared, unless another's typed by then.
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(trimmed), Some(from)) =
            (submitted_name(&self.name, cx), self.from.get().cloned())
        else {
            return;
        };
        if !self.open || self.repository.read(cx).branches_changing() {
            return;
        }
        self.close(window, cx);
        let input = NewBranchFrom {
            name: trimmed.clone(),
            from,
        };
        let done = self.repository.update(cx, |repository, cx| {
            repository.create_branch_from(input, cx)
        });
        let Some(done) = done else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(())) = done.await {
                let _ = this.update_in(cx, |this, window, cx| {
                    // Only the name that was used: another may be being typed in another
                    // repository by then.
                    if this.name.read(cx).value().trim() == trimmed {
                        this.name
                            .update(cx, |name, cx| name.set_value("", window, cx));
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
    use gitto_git::features::status::schema::Head;
    use gitto_git::input::{BranchName, BranchRef};
    use gpui_kit::component::WindowExt as _;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AnyWindowHandle, IntoElement, Render, TestAppContext, div};

    use super::*;
    use crate::components::repository::test_support::{
        changed, fake_git, repository_id, set_status, setup, status,
    };
    use gitto_app_data::data::git::{GitTask, RepositoryGit};

    /// A repository's git that records the branches asked to be created.
    #[derive(Default)]
    struct Creates(RefCell<Vec<NewBranchFrom>>);

    impl RepositoryGit for Creates {
        fn create_branch(&self, name: BranchName, from: Option<BranchRef>) -> GitTask<()> {
            self.0.borrow_mut().push(NewBranchFrom {
                name: name.as_str().to_owned(),
                from: from
                    .map(|from| from.as_str().to_owned())
                    .unwrap_or_default(),
            });
            async { Ok(()) }.boxed()
        }
    }

    struct Host {
        dialog: Entity<CreateBranchDialog>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    fn render(
        cx: &mut TestAppContext,
        id: &RepositoryId,
        calls: &Rc<Creates>,
    ) -> (AnyWindowHandle, Entity<CreateBranchDialog>) {
        fake_git(id, calls.clone(), cx);
        let id = id.clone();
        let (window, host) = crate::test_support::open(cx, move |window, cx| Host {
            dialog: cx.new(|cx| CreateBranchDialog::new(id, window, cx)),
        });
        let dialog = host.read_with(cx, |host, _| host.dialog.clone());
        (window, dialog)
    }

    #[gpui_kit::test]
    fn names_a_branch_from_another_taking_the_changes_along(cx: &mut TestAppContext) {
        let _dir = setup(cx);
        let id = repository_id();
        set_status(&id, changed(status(), 2), cx);
        let calls = Rc::new(Creates::default());
        let (window, dialog) = render(cx, &id, &calls);

        cx.update_window(window, |_, window, cx| {
            dialog.update(cx, |dialog, cx| {
                dialog.open("refs/remotes/origin/feature".into(), window, cx)
            })
        })
        .unwrap();
        cx.run_until_parked();
        assert!(
            cx.update_window(window, |_, window, cx| window.has_active_dialog(cx))
                .unwrap()
        );
        assert_eq!(
            dialog.read_with(cx, |dialog, cx| dialog.summary(cx)),
            "From origin/feature, with your uncommitted changes, unless they conflict with it: \
             then they're kept in the stash."
        );

        cx.update_window(window, |_, window, cx| {
            window.input("topic", cx);
            window.press("enter", cx);
        })
        .unwrap();
        cx.run_until_parked();
        assert_eq!(
            *calls.0.borrow(),
            [NewBranchFrom {
                name: "topic".into(),
                from: "refs/remotes/origin/feature".into(),
            }]
        );
        assert!(!dialog.read_with(cx, |dialog, _| dialog.is_open()));
        // Created: the next one gets a fresh name.
        assert_eq!(
            dialog.read_with(cx, |dialog, cx| dialog.name.read(cx).value().to_string()),
            ""
        );
    }

    #[gpui_kit::test]
    fn says_the_changes_come_along_from_the_checked_out_branch(cx: &mut TestAppContext) {
        let _dir = setup(cx);
        let id = repository_id();
        let mut on_main = changed(status(), 1);
        on_main.head = Head::Branch {
            name: "main".into(),
            sha: "abc".into(),
        };
        set_status(&id, on_main, cx);
        let calls = Rc::new(Creates::default());
        let (window, dialog) = render(cx, &id, &calls);
        cx.update_window(window, |_, window, cx| {
            dialog.update(cx, |dialog, cx| {
                dialog.open("refs/heads/main".into(), window, cx)
            })
        })
        .unwrap();
        assert_eq!(
            dialog.read_with(cx, |dialog, cx| dialog.summary(cx)),
            "From main, with your uncommitted changes."
        );
    }
}
