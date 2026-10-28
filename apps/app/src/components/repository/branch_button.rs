//! The toolbar's Branch. Ported from `components/repository/branch-button.tsx`.

use gitto_git::input::RepositoryId;
use gitto_ui_logic::git::status::{branch_source, has_uncommitted_changes};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::popover::Popover;
use gpui_kit::prelude::*;
use gpui_kit::{Anchor, App, Context, Entity, Focusable as _, Subscription, Window, div, rems};

use super::branch_form::{BranchForm, NAME_PLACEHOLDER, submitted_name};
use super::toolbar_button::{HideBelow, ToolbarButton};
use gitto_app_data::data::changes::CREATE_BRANCH;
use gitto_app_data::data::{Part, Shown};
use gitto_app_kit::ui::IconName;

/// Creates a branch from the current one and switches to it, with a popover to name it, and says
/// why if that failed. Uncommitted changes come along to the new branch. An entity: it keeps the
/// popover's name, which is kept after a failure to fix it, and is for the repository it was
/// typed in: another repository starts with none, and the popover closed.
pub struct BranchButton {
    repository: Shown,
    name: Entity<InputState>,
    open: bool,
    _subscriptions: Vec<Subscription>,
}

impl BranchButton {
    pub fn new(repository_id: RepositoryId, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder(NAME_PLACEHOLDER));
        let subscriptions = vec![
            cx.subscribe_in(&name, window, |this, _, event, window, cx| match event {
                InputEvent::PressEnter { .. } => this.submit(window, cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            }),
        ];
        BranchButton {
            repository: Shown::new(&repository_id, &[Part::Status, Part::Operations], cx),
            name,
            open: false,
            _subscriptions: subscriptions,
        }
    }

    /// Shows another repository's: the popover names a branch for the repository it was opened
    /// in.
    pub fn set_repository(
        &mut self,
        repository_id: RepositoryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.repository.set(&repository_id, cx) {
            self.open = false;
            self.name
                .update(cx, |name, cx| name.set_value("", window, cx));
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// What the new branch is made from, and what comes along.
    pub fn summary(&self, cx: &App) -> String {
        let status = self
            .repository
            .read(cx)
            .status()
            .data()
            .map(|status| status.summary());
        let source = status
            .as_ref()
            .map_or("HEAD", |status| branch_source(&status.head));
        let changes = if has_uncommitted_changes(status.as_ref()) {
            ", with your uncommitted changes."
        } else {
            "."
        };
        format!("From {source}{changes}")
    }

    /// Opens or closes the popover. Opening it gets why the last one failed out of the way of
    /// the name, which is kept after a failure to fix it.
    pub fn toggle(&mut self, open: bool, cx: &mut Context<Self>) {
        if open {
            self.repository
                .update(cx, |repository, cx| repository.dismiss(CREATE_BRANCH, cx));
        }
        self.open = open;
        cx.notify();
    }

    /// Creates the branch named in the popover, which closes: the button shows it running, and
    /// why it failed. Once it's created, the name's cleared, unless another's typed by then.
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(trimmed) = submitted_name(&self.name, cx) else {
            return;
        };
        if !self.open || self.repository.read(cx).branches_changing() {
            return;
        }
        self.open = false;
        cx.notify();
        let name = trimmed.to_string();
        let done = self
            .repository
            .update(cx, |repository, cx| repository.create_branch(name, cx));
        let Some(done) = done else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(())) = done.await {
                let _ = this.update_in(cx, |this, window, cx| {
                    // Only the name that was used: another may be being typed by then.
                    if this.name.read(cx).value().trim() == trimmed {
                        this.name
                            .update(cx, |name, cx| name.set_value("", window, cx));
                    }
                });
            }
        })
        .detach();
    }

    /// The button as it is now. Until the status has loaded, it's disabled.
    pub fn button(&self, cx: &App) -> ToolbarButton {
        let repository = self.repository.read(cx);
        let state = repository.operation_state(CREATE_BRANCH);
        let status = repository.status().data();
        let source = status.map_or("HEAD", |status| branch_source(&status.head));
        let dismiss = self.repository.entity().clone();
        ToolbarButton::new("branch", IconName::GitBranch, "Branch")
            .hide_below(HideBelow::Small)
            .title(format!("Create a branch from {source}"))
            .disabled(status.is_none() || repository.branches_changing())
            .busy(state.running)
            .failure("Branch", state.error, move |_, cx| {
                dismiss.update(cx, |repository, cx| repository.dismiss(CREATE_BRANCH, cx))
            })
    }
}

impl Render for BranchButton {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let opening = cx.entity().downgrade();
        // The failure popover around the button takes the press, so the button opens this one
        // itself.
        let button = self.button(cx).on_click(move |_, _, cx| {
            let _ = opening.update(cx, |this, cx| this.toggle(true, cx));
        });
        // Left out with the button, which can't anchor it then.
        if button.is_hidden_at(f32::from(window.viewport_size().width)) {
            return div().into_any_element();
        }
        let disabled = button.is_disabled() || button.is_busy();
        let this = cx.entity().downgrade();
        let form = cx.entity();
        Popover::new("branch-popover")
            .anchor(Anchor::TopLeft)
            .open(self.open && !disabled)
            .track_focus(&self.name.focus_handle(cx))
            .on_open_change(move |open, _, cx| {
                let open = *open;
                let _ = this.update(cx, |this, cx| this.toggle(open, cx));
            })
            .trigger(button)
            .content(move |_, _, cx| {
                let form = form.clone();
                let (summary, name) = {
                    let button = form.read(cx);
                    (button.summary(cx), button.name.clone())
                };
                div()
                    .w(rems(18.67))
                    .child(
                        BranchForm::new(summary, &name).on_submit(move |_, window, cx| {
                            form.update(cx, |this, cx| this.submit(window, cx));
                        }),
                    )
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use futures::FutureExt as _;
    use futures::channel::oneshot;
    use gitto_git::features::status::schema::Head;
    use gitto_git::input::{BranchName, BranchRef};
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AnyWindowHandle, TestAppContext};

    use super::*;
    use crate::components::repository::test_support::{
        changed, fake_git, repository_id, set_status, setup, status, uncommitted,
    };
    use gitto_app_data::data::DataError;
    use gitto_app_data::data::git::{GitTask, RepositoryGit};

    /// A repository's git that records the branches asked to be created, and what settles each.
    #[derive(Default)]
    struct Creates {
        calls: RefCell<Vec<String>>,
        settle: RefCell<Vec<oneshot::Sender<Result<(), DataError>>>>,
    }

    impl RepositoryGit for Creates {
        fn create_branch(&self, name: BranchName, _: Option<BranchRef>) -> GitTask<()> {
            self.calls.borrow_mut().push(name.as_str().to_owned());
            let (settle, settled) = oneshot::channel();
            self.settle.borrow_mut().push(settle);
            async move { settled.await.unwrap_or(Err(DataError::msg("dropped"))) }.boxed()
        }
    }

    fn render(
        cx: &mut TestAppContext,
        id: &RepositoryId,
        creates: &Rc<Creates>,
    ) -> (AnyWindowHandle, Entity<BranchButton>) {
        fake_git(id, creates.clone(), cx);
        let id = id.clone();
        crate::test_support::open(cx, move |window, cx| BranchButton::new(id, window, cx))
    }

    fn click(window: AnyWindowHandle, id: &'static str, cx: &mut TestAppContext) {
        cx.update_window(window, |_, window, cx| window.click(id, cx))
            .unwrap();
        cx.run_until_parked();
    }

    fn typed(window: AnyWindowHandle, text: &str, cx: &mut TestAppContext) {
        cx.update_window(window, |_, window, cx| window.input(text, cx))
            .unwrap();
        cx.run_until_parked();
    }

    fn press(window: AnyWindowHandle, key: &str, cx: &mut TestAppContext) {
        cx.update_window(window, |_, window, cx| window.press(key, cx))
            .unwrap();
        cx.run_until_parked();
    }

    fn name(button: &Entity<BranchButton>, cx: &mut TestAppContext) -> String {
        button.read_with(cx, |button, cx| button.name.read(cx).value().to_string())
    }

    #[gpui_kit::test]
    fn creates_a_branch_from_the_current_one_with_the_changes(cx: &mut TestAppContext) {
        let _dir = setup(cx);
        let id = repository_id();
        set_status(&id, changed(status(), 2), cx);
        let creates = Rc::new(Creates::default());
        let (window, button) = render(cx, &id, &creates);
        assert_eq!(
            button.read_with(cx, |button, cx| button.button(cx).tooltip().clone()),
            "Create a branch from main"
        );

        click(window, "branch", cx);
        assert!(button.read_with(cx, |button, _| button.is_open()));
        assert_eq!(
            button.read_with(cx, |button, cx| button.summary(cx)),
            "From main, with your uncommitted changes."
        );

        // The name field has focus once the popover opens.
        typed(window, "  feature/new  ", cx);
        press(window, "enter", cx);
        assert_eq!(*creates.calls.borrow(), ["feature/new"]);
        // The popover closes, and the button shows it running.
        assert!(!button.read_with(cx, |button, _| button.is_open()));
        let shown = button.read_with(cx, |button, cx| button.button(cx));
        assert!(shown.is_busy());

        let mut moved = changed(status(), 2);
        moved.head = Head::Branch {
            name: "feature/new".into(),
            sha: "abc".into(),
        };
        moved.version = "v2".into();
        set_status(&id, moved, cx);
        let settle = creates.settle.borrow_mut().remove(0);
        settle.send(Ok(())).unwrap();
        cx.run_until_parked();
        let shown = button.read_with(cx, |button, cx| button.button(cx));
        assert_eq!(shown.tooltip(), "Create a branch from feature/new");
        assert!(!shown.is_busy());
        assert!(!shown.is_disabled());

        // The next branch gets a fresh name.
        assert_eq!(name(&button, cx), "");
    }

    #[gpui_kit::test]
    fn says_why_a_branch_couldnt_be_created_and_keeps_the_name_to_fix(cx: &mut TestAppContext) {
        let _dir = setup(cx);
        let id = repository_id();
        set_status(&id, changed(status(), 2), cx);
        let creates = Rc::new(Creates::default());
        let (window, button) = render(cx, &id, &creates);

        click(window, "branch", cx);
        typed(window, "topic", cx);
        click(window, "create-branch", cx);
        let settle = creates.settle.borrow_mut().remove(0);
        settle
            .send(Err(DataError::msg(
                "fatal: cannot switch branch while merging",
            )))
            .unwrap();
        cx.run_until_parked();
        let failure = button.read_with(cx, |button, cx| {
            button.button(cx).shown_failure().map(DataError::to_string)
        });
        assert_eq!(
            failure.as_deref(),
            Some("fatal: cannot switch branch while merging")
        );

        click(window, "branch", cx);
        assert_eq!(name(&button, cx), "topic");
        // Out of the way of the name.
        let failure = button.read_with(cx, |button, cx| button.button(cx).shown_failure().cloned());
        assert!(failure.is_none());
    }

    #[gpui_kit::test]
    fn keeps_a_creation_with_the_repository_its_in(cx: &mut TestAppContext) {
        let _dir = setup(cx);
        let a = repository_id();
        let b = repository_id();
        set_status(&a, changed(status(), 2), cx);
        set_status(&b, changed(status(), 2), cx);
        let creates = Rc::new(Creates::default());
        let (window, button) = render(cx, &a, &creates);

        click(window, "branch", cx);
        typed(window, "topic", cx);
        press(window, "enter", cx);
        assert!(button.read_with(cx, |button, cx| button.button(cx).is_busy()));

        // Another repository's button can be used meanwhile, and opening it changes nothing in
        // "a".
        cx.update_window(window, |_, window, cx| {
            button.update(cx, |button, cx| {
                button.set_repository(b.clone(), window, cx)
            })
        })
        .unwrap();
        let shown = button.read_with(cx, |button, cx| button.button(cx));
        assert!(!shown.is_busy());
        assert!(!shown.is_disabled());
        click(window, "branch", cx);
        assert!(button.read_with(cx, |button, _| button.is_open()));
        press(window, "escape", cx);

        cx.update_window(window, |_, window, cx| {
            button.update(cx, |button, cx| {
                button.set_repository(a.clone(), window, cx)
            })
        })
        .unwrap();
        assert!(button.read_with(cx, |button, cx| button.button(cx).is_busy()));
        let settle = creates.settle.borrow_mut().remove(0);
        settle
            .send(Err(DataError::msg(
                "fatal: a branch named 'topic' already exists",
            )))
            .unwrap();
        cx.run_until_parked();
        let failure = button.read_with(cx, |button, cx| {
            button.button(cx).shown_failure().map(DataError::to_string)
        });
        assert_eq!(
            failure.as_deref(),
            Some("fatal: a branch named 'topic' already exists")
        );
    }

    #[gpui_kit::test]
    fn names_the_commit_its_made_from_with_head_detached(cx: &mut TestAppContext) {
        let _dir = setup(cx);
        let id = repository_id();
        set_status(&id, uncommitted(Head::Detached { sha: "abc".into() }), cx);
        let creates = Rc::new(Creates::default());
        let (_, button) = render(cx, &id, &creates);
        assert_eq!(
            button.read_with(cx, |button, cx| button.button(cx).tooltip().clone()),
            "Create a branch from this commit"
        );
        assert_eq!(
            button.read_with(cx, |button, cx| button.summary(cx)),
            "From this commit."
        );
    }
}
