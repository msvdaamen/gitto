//! The toolbar's Push, and what to do when the remote branch turns a push down. Ported from
//! `components/repository/push-button.tsx`.

use gitto_git::features::remote::schema::PushTarget;
use gitto_git::input::RepositoryId;
use gitto_ui::git::push::push_title;
use gitto_ui::git::status::head_pull_blocker;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, SharedString, Subscription, Window, div};

use super::toolbar_button::ToolbarButton;
use crate::components::ui::dialog::CONFIRM_WIDTH;
use crate::components::ui::{DialogHeading, IconName, Tone};
use crate::data::changes::PUSH;
use crate::data::remote::{PushRejection, push_rejection};
use crate::data::{Changed, Part, Repository, Shown};
use crate::state::lingering::Lingering;

/// Why the remote branch turned a push down, and how git put it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    pub kind: PushRejection,
    pub message: String,
}

/// What the dialog about a rejected push says and offers (see [`PushButton::rejected`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedCopy {
    pub description: String,
    /// The button that leaves the remote alone: "Cancel" when something else is offered, "Close"
    /// when nothing is.
    pub cancel: &'static str,
    /// Whether Pull is offered, and as the suggestion (primary) rather than the alternative.
    pub pull: Option<ButtonVariant>,
    pub force_push: bool,
}

impl RejectedCopy {
    /// What's said about `rejection`: force-pushing when the remote branch has the commits from
    /// before a rebase or an amend, which pulling would bring back; pulling (if it can, see
    /// [`PushButton::can_pull`]) when it has someone else's, which force-pushing wouldn't replace.
    pub fn of(rejection: &Rejection, can_pull: bool) -> Self {
        let rewritten = rejection.kind == PushRejection::Rewritten;
        let advice = if rewritten {
            Some("Force-push to replace them.")
        } else if can_pull {
            Some("Pull them first.")
        } else {
            None
        };
        RejectedCopy {
            description: match advice {
                Some(advice) => format!("{} {advice}", rejection.message),
                None => rejection.message.clone(),
            },
            cancel: if rewritten || can_pull {
                "Cancel"
            } else {
                "Close"
            },
            pull: can_pull.then_some(if rewritten {
                ButtonVariant::Secondary
            } else {
                ButtonVariant::Primary
            }),
            force_push: rewritten,
        }
    }
}

/// Pushes the current branch, and says why if that failed. When the remote branch has commits
/// the branch doesn't, asks what to do instead: force-push, or pull, whichever suits. An entity: it
/// shows where it pushes (and how many commits) and the push's state, of the repository it's
/// given; the dialog is open while that repository's push was turned down, so it's there again
/// when switching back to it.
pub struct PushButton {
    repository: Shown,
    /// What the dialog shows: the rejection, still, while the dialog closes.
    shown: Lingering<Rejection>,
    dialog_open: bool,
    /// Opens and closes the dialog as the repository's push is turned down, and that's dismissed.
    _push: Subscription,
}

impl PushButton {
    pub fn new(repository_id: RepositoryId, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let repository = Shown::new(
            &repository_id,
            &[Part::Status, Part::PushTarget, Part::Operations],
            cx,
        );
        let mut button = PushButton {
            _push: Self::follow_push(repository.entity(), window, cx),
            repository,
            shown: Lingering::default(),
            dialog_open: false,
        };
        button.sync_dialog(window, cx);
        button
    }

    fn follow_push(
        repository: &Entity<Repository>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe_in(repository, window, |this, _, Changed(part), window, cx| {
            if *part == Part::Operations {
                this.sync_dialog(window, cx);
            }
        })
    }

    /// Shows another repository's.
    pub fn set_repository(
        &mut self,
        repository_id: RepositoryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.repository.set(&repository_id, cx) {
            self._push = Self::follow_push(self.repository.entity(), window, cx);
            self.sync_dialog(window, cx);
        }
    }

    /// Why the remote branch turned the last push down, if it did: asked about in the dialog
    /// rather than shown as a failure.
    pub fn rejected(&self, cx: &App) -> Option<Rejection> {
        let error = self.repository.read(cx).operation_state(PUSH).error?;
        let kind = push_rejection(Some(&error))?;
        Some(Rejection {
            kind,
            message: error.message(),
        })
    }

    /// Whether pulling would bring in the remote branch's commits: it's the one pulled from, and
    /// the branch can be pulled.
    pub fn can_pull(&self, cx: &App) -> bool {
        let repository = self.repository.read(cx);
        let pullable = repository.status().data().is_some_and(|status| {
            head_pull_blocker(&status.head, status.upstream.as_deref()).is_none()
        });
        let pulls_from = matches!(
            repository.push_target().data(),
            Some(PushTarget::Remote {
                pulls_from: true,
                ..
            })
        );
        pullable && pulls_from
    }

    /// Opens the dialog while the push was turned down, and closes it once that's no longer so
    /// (it was cancelled, or another repository is on show).
    fn sync_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rejected = self.rejected(cx);
        let wanted = rejected.is_some();
        self.shown.update(rejected);
        if wanted && !self.dialog_open {
            self.dialog_open = true;
            let this = cx.entity();
            window.open_dialog(cx, move |dialog, window, cx| {
                let copy = this.read(cx).rejected_copy(cx);
                let cancel = this.clone();
                dialog
                    .w(CONFIRM_WIDTH.to_pixels(window.rem_size()))
                    .close_button(false)
                    .on_cancel(move |_, _, cx| {
                        cancel.update(cx, |this, cx| this.cancel_rejected(cx));
                        true
                    })
                    .child(
                        DialogHeading::new(IconName::TriangleAlert, Tone::Amber, "Push rejected")
                            .description(div().child(copy.description.clone())),
                    )
                    .footer(Self::rejected_footer(&this, &copy))
            });
        } else if !wanted && self.dialog_open {
            self.dialog_open = false;
            window.close_dialog(cx);
        }
    }

    /// What the dialog says now, of the rejection it's about.
    fn rejected_copy(&self, cx: &App) -> RejectedCopy {
        let rejection = self.rejected(cx).or_else(|| self.shown.get().cloned());
        let rejection = rejection.unwrap_or(Rejection {
            kind: PushRejection::Behind,
            message: String::new(),
        });
        RejectedCopy::of(&rejection, self.can_pull(cx))
    }

    fn rejected_footer(this: &Entity<Self>, copy: &RejectedCopy) -> DialogFooter {
        let cancel = this.clone();
        let mut footer = DialogFooter::new().gap_2().child(
            Button::new("cancel")
                .ghost()
                .label(copy.cancel)
                .on_click(move |_, window, cx| {
                    window.close_dialog(cx);
                    cancel.update(cx, |this, cx| this.cancel_rejected(cx));
                }),
        );
        if let Some(variant) = copy.pull {
            let pull = this.clone();
            footer = footer.child(
                Button::new("pull-instead")
                    .with_variant(variant)
                    .label("Pull")
                    .on_click(move |_, window, cx| {
                        window.close_dialog(cx);
                        pull.update(cx, |this, cx| this.pull_instead(cx));
                    }),
            );
        }
        if copy.force_push {
            let force = this.clone();
            footer = footer.child(
                Button::new("force-push")
                    .primary()
                    .label("Force push")
                    .on_click(move |_, window, cx| {
                        window.close_dialog(cx);
                        force.update(cx, |this, cx| this.force_push(cx));
                    }),
            );
        }
        footer
    }

    /// Leaves the remote alone.
    fn cancel_rejected(&mut self, cx: &mut Context<Self>) {
        self.dialog_open = false;
        self.repository
            .update(cx, |repository, cx| repository.dismiss(PUSH, cx));
    }

    /// Pulls the remote branch's commits instead.
    fn pull_instead(&mut self, cx: &mut Context<Self>) {
        self.dialog_open = false;
        self.repository.update(cx, |repository, cx| {
            repository.dismiss(PUSH, cx);
            drop(repository.pull(cx));
        });
    }

    /// Replaces the remote branch's commits.
    fn force_push(&mut self, cx: &mut Context<Self>) {
        self.dialog_open = false;
        self.repository
            .update(cx, |repository, cx| drop(repository.push(true, cx)));
    }

    /// The button as it is now. Until where it pushes has loaded, it's disabled.
    pub fn button(&self, cx: &App) -> ToolbarButton {
        let repository = self.repository.read(cx);
        let state = repository.operation_state(PUSH);
        let target = repository.push_target().data();
        let title: SharedString = match (&target, repository.push_target().error()) {
            (Some(target), _) => push_title(target).into(),
            (None, Some(error)) => {
                format!("Couldn't tell where to push: {}", error.message()).into()
            }
            (None, None) => "Push".into(),
        };
        let (pushable, ahead) = match &target {
            Some(PushTarget::Remote { ahead, .. }) => (true, Some(*ahead)),
            _ => (false, None),
        };
        // A rejection is asked about in the dialog rather than shown as a failure.
        let failure = state
            .error
            .filter(|error| push_rejection(Some(error)).is_none());
        let push = self.repository.entity().clone();
        let dismiss = push.clone();
        ToolbarButton::new("push", IconName::Upload, "Push")
            .title(title)
            .disabled(!pushable || state.running)
            .busy(state.running)
            .count(ahead)
            .failure("Push", failure, move |_, cx| {
                dismiss.update(cx, |repository, cx| repository.dismiss(PUSH, cx))
            })
            .on_click(move |_, _, cx| {
                push.update(cx, |repository, cx| drop(repository.push(false, cx)));
            })
    }
}

impl Render for PushButton {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.button(cx)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use futures::FutureExt as _;
    use gitto_git::features::remote::schema::PushTarget;
    use gitto_git::input::NonEmpty;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AnyWindowHandle, TestAppContext};

    use super::*;
    use crate::components::repository::test_support::{
        fake_git, has_dialog, repository_id, set_push_target, set_status, setup, status,
    };
    use crate::data::QueryError;
    use crate::data::git::{GitTask, RepositoryGit};

    const REWRITTEN: &str = "origin/main still has main's commits from before a rebase or amend.";
    const BEHIND: &str = "origin/main has commits you haven't pulled yet.";

    fn target() -> PushTarget {
        PushTarget::Remote {
            name: "origin/main".into(),
            is_new: false,
            ahead: 2,
            pulls_from: true,
        }
    }

    /// A push turned down as the UI gets it (see `push_rejection`).
    fn rejected(forcible: bool) -> QueryError {
        QueryError::from(gitto_git::Error::PushRejected {
            message: if forcible { REWRITTEN } else { BEHIND }.to_owned(),
            forcible,
        })
    }

    type Answer = Box<dyn Fn(bool, usize) -> Result<(), QueryError>>;

    /// A repository's git that records the pushes asked for (whether forced), each answered by
    /// `answer`, and the pulls.
    struct Calls {
        pushes: RefCell<Vec<bool>>,
        pulls: RefCell<usize>,
        answer: Answer,
    }

    impl RepositoryGit for Calls {
        fn push(&self, force: bool) -> GitTask<()> {
            self.pushes.borrow_mut().push(force);
            let result = (self.answer)(force, self.pushes.borrow().len());
            async move { result }.boxed()
        }

        fn pull(&self, _: Option<NonEmpty>) -> GitTask<()> {
            *self.pulls.borrow_mut() += 1;
            async { Ok(()) }.boxed()
        }
    }

    fn operations(answer: impl Fn(bool, usize) -> Result<(), QueryError> + 'static) -> Rc<Calls> {
        Rc::new(Calls {
            pushes: RefCell::new(Vec::new()),
            pulls: RefCell::new(0),
            answer: Box::new(answer),
        })
    }

    fn render(
        cx: &mut TestAppContext,
        target: PushTarget,
        calls: &Rc<Calls>,
    ) -> (tempfile::TempDir, AnyWindowHandle, Entity<PushButton>) {
        let dir = setup(cx);
        let id = repository_id();
        let mut summary = status();
        summary.ahead = 2;
        summary.behind = 1;
        set_status(&id, summary, cx);
        set_push_target(&id, target, cx);
        fake_git(&id, calls.clone(), cx);
        let (window, button) =
            crate::test_support::open(cx, move |window, cx| PushButton::new(id, window, cx));
        (dir, window, button)
    }

    fn click(window: AnyWindowHandle, id: &'static str, cx: &mut TestAppContext) {
        cx.update_window(window, |_, window, cx| window.click(id, cx))
            .unwrap();
        cx.run_until_parked();
    }

    fn copy(button: &Entity<PushButton>, cx: &mut TestAppContext) -> RejectedCopy {
        button.read_with(cx, |button, cx| button.rejected_copy(cx))
    }

    #[gpui_kit::test]
    fn pushes_and_shows_how_many_commits_there_are_to_push(cx: &mut TestAppContext) {
        let calls = operations(|_, _| Ok(()));
        let (_dir, window, button) = render(cx, target(), &calls);

        let shown = button.read_with(cx, |button, cx| button.button(cx));
        assert_eq!(shown.tooltip(), "Push 2 commits to origin/main");
        assert_eq!(shown.shown_count(), Some(2));
        click(window, "push", cx);
        assert_eq!(*calls.pushes.borrow(), [false]);
    }

    #[gpui_kit::test]
    fn offers_to_force_push_when_the_remote_has_the_commits_from_before_a_rebase(
        cx: &mut TestAppContext,
    ) {
        let calls = operations(|_, call| {
            if call == 1 {
                Err(rejected(true))
            } else {
                Ok(())
            }
        });
        let (_dir, window, button) = render(cx, target(), &calls);

        click(window, "push", cx);
        assert!(has_dialog(window, cx));
        let copy = copy(&button, cx);
        assert_eq!(
            copy.description,
            format!("{REWRITTEN} Force-push to replace them.")
        );
        // Pulling is there too, but force-pushing is what's suggested.
        assert_eq!(copy.pull, Some(ButtonVariant::Secondary));
        assert!(copy.force_push);

        click(window, "force-push", cx);
        assert_eq!(*calls.pushes.borrow(), [false, true]);
        assert!(!has_dialog(window, cx));
    }

    #[gpui_kit::test]
    fn offers_to_pull_and_not_to_force_push_over_someone_elses_commits(cx: &mut TestAppContext) {
        let calls = operations(|_, _| Err(rejected(false)));
        let (_dir, window, button) = render(cx, target(), &calls);

        click(window, "push", cx);
        assert!(has_dialog(window, cx));
        let copy = copy(&button, cx);
        assert_eq!(copy.description, format!("{BEHIND} Pull them first."));
        assert!(!copy.force_push);

        click(window, "pull-instead", cx);
        assert_eq!(*calls.pulls.borrow(), 1);
        assert!(!has_dialog(window, cx));
        assert_eq!(calls.pushes.borrow().len(), 1);
    }

    #[gpui_kit::test]
    fn doesnt_offer_to_pull_from_another_branch_than_it_pushes_to(cx: &mut TestAppContext) {
        let calls = operations(|_, _| Err(rejected(false)));
        let fork = PushTarget::Remote {
            name: "fork/main".into(),
            is_new: false,
            ahead: 2,
            pulls_from: false,
        };
        let (_dir, window, button) = render(cx, fork, &calls);
        assert_eq!(
            button.read_with(cx, |button, cx| button.button(cx).tooltip().clone()),
            "Push 2 commits to fork/main"
        );

        click(window, "push", cx);
        let copy = copy(&button, cx);
        assert_eq!(copy.description, BEHIND);
        assert_eq!(copy.pull, None);
        assert_eq!(copy.cancel, "Close");
        click(window, "cancel", cx);
        assert!(!has_dialog(window, cx));
        assert_eq!(*calls.pulls.borrow(), 0);
    }

    #[gpui_kit::test]
    fn leaves_the_remote_alone_when_cancelled(cx: &mut TestAppContext) {
        let calls = operations(|_, _| Err(rejected(true)));
        let (_dir, window, button) = render(cx, target(), &calls);

        click(window, "push", cx);
        assert!(has_dialog(window, cx));
        click(window, "cancel", cx);

        assert!(!has_dialog(window, cx));
        assert_eq!(calls.pushes.borrow().len(), 1);
        assert_eq!(*calls.pulls.borrow(), 0);
        // Nor shown as a failure.
        let shown = button.read_with(cx, |button, cx| button.button(cx));
        assert!(shown.shown_failure().is_none());
    }

    #[gpui_kit::test]
    fn says_why_a_push_failed_otherwise(cx: &mut TestAppContext) {
        let calls = operations(|_, _| Err(QueryError::msg("Permission denied (publickey).")));
        let (_dir, window, button) = render(cx, target(), &calls);

        click(window, "push", cx);
        let shown = button.read_with(cx, |button, cx| button.button(cx));
        assert_eq!(
            shown.shown_failure().map(QueryError::message).as_deref(),
            Some("Permission denied (publickey).")
        );
        assert!(!has_dialog(window, cx));
    }

    #[gpui_kit::test]
    fn says_when_it_pushes_to_a_new_remote_branch(cx: &mut TestAppContext) {
        let calls = operations(|_, _| Ok(()));
        let new = PushTarget::Remote {
            name: "origin/feature".into(),
            is_new: true,
            ahead: 2,
            pulls_from: true,
        };
        let (_dir, _, button) = render(cx, new, &calls);
        let shown = button.read_with(cx, |button, cx| button.button(cx));
        assert_eq!(
            shown.tooltip(),
            "Push to origin/feature, a new remote branch"
        );
        assert!(!shown.is_disabled());
    }

    #[gpui_kit::test]
    fn is_disabled_saying_why_when_theres_nothing_to_push_to(cx: &mut TestAppContext) {
        let calls = operations(|_, _| Ok(()));
        let blocked = PushTarget::Blocked {
            reason: "There's no remote to push to.".into(),
        };
        let (_dir, _, button) = render(cx, blocked, &calls);
        let shown = button.read_with(cx, |button, cx| button.button(cx));
        assert_eq!(shown.tooltip(), "There's no remote to push to.");
        assert!(shown.is_disabled());
    }
}
