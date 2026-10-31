//! The toolbar's Pull. Ported from `components/repository/pull-button.tsx`.

use gitto_git::features::status::schema::{StatusSummary, Uncommitted};
use gitto_git::input::RepositoryId;
use gitto_ui::git::status::{head_pull_blocker, pull_title};
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Subscription, Window};

use super::toolbar_button::ToolbarButton;
use crate::components::ui::IconName;
use crate::data::remote::{PullOperation, pull_operation};
use crate::data::status::status_query;
use crate::data::{Opaque, Operations, Query};

/// Pulls the current branch's upstream, and says why if that failed or was left to finish. An
/// entity: it shows the status (how many commits there are to pull, and why it can't) and the
/// pull's state, of the repository it's given. The pull is the repository's, wherever it was
/// started (a branch's menu pulls the checked-out one too): switching away and back shows it
/// still running, or why it failed.
pub struct PullButton {
    repository_id: RepositoryId,
    status: Query<Opaque<Uncommitted>, StatusSummary>,
    pull: PullOperation,
    _subscriptions: Vec<Subscription>,
}

impl PullButton {
    pub fn new(repository_id: RepositoryId, _: &mut Window, cx: &mut Context<Self>) -> Self {
        PullButton::with_pull(repository_id, pull_operation(), cx)
    }

    /// With `pull` doing the pulling: a test's.
    pub(crate) fn with_pull(
        repository_id: RepositoryId,
        pull: PullOperation,
        cx: &mut Context<Self>,
    ) -> Self {
        PullButton {
            status: status_query(&repository_id, cx),
            repository_id,
            pull,
            _subscriptions: vec![cx.observe_global::<Operations>(|_, cx| cx.notify())],
        }
    }

    pub fn repository_id(&self) -> &RepositoryId {
        &self.repository_id
    }

    /// The button as it is now. Until the status has loaded, it's disabled.
    pub fn button(&self, cx: &App) -> ToolbarButton {
        let id = self.repository_id.clone();
        let state = self.pull.state(&id, cx);
        let status = self.status.data();
        let title = status.as_ref().map_or_else(
            || "Pull".to_owned(),
            |status| pull_title(&status.head, status.upstream.as_deref(), status.behind),
        );
        let blocked = status.as_ref().is_none_or(|status| {
            head_pull_blocker(&status.head, status.upstream.as_deref()).is_some()
        });
        let pull = self.pull.clone();
        let dismiss = self.pull.clone();
        let dismissed = id.clone();
        ToolbarButton::new("pull", IconName::Download, "Pull")
            .accent(true)
            .title(title)
            .disabled(blocked || state.running)
            .busy(state.running)
            .count(status.map(|status| status.behind))
            .failure("Pull", state.error, move |_, cx| {
                dismiss.dismiss(&dismissed, cx)
            })
            .on_click(move |_, _, cx| {
                let _ = pull.pull(&id, cx);
            })
    }
}

impl Render for PullButton {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.button(cx)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use futures::channel::oneshot;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};

    use super::*;
    use crate::components::repository::test_support::{repository_id, set_status, setup, status};
    use crate::data::{QueryError, RepositoryOperation};

    /// A pull that the test settles, and the repositories it was asked to pull.
    struct Pulls {
        calls: RefCell<Vec<String>>,
        settle: RefCell<Vec<oneshot::Sender<Result<(), QueryError>>>>,
    }

    fn pulls() -> (Rc<Pulls>, PullOperation) {
        let pulls = Rc::new(Pulls {
            calls: RefCell::new(Vec::new()),
            settle: RefCell::new(Vec::new()),
        });
        let operation = {
            let pulls = Rc::clone(&pulls);
            RepositoryOperation::new("pull", move |id: RepositoryId, (), cx: &mut App| {
                pulls.calls.borrow_mut().push(id.to_string());
                let (settle, settled) = oneshot::channel();
                pulls.settle.borrow_mut().push(settle);
                cx.background_spawn(async move {
                    settled.await.unwrap_or(Err(QueryError::msg("dropped")))
                })
            })
        };
        (pulls, PullOperation::with_operation(operation))
    }

    fn render(
        cx: &mut TestAppContext,
        id: &RepositoryId,
        pull: PullOperation,
    ) -> (AnyWindowHandle, Entity<PullButton>) {
        let id = id.clone();
        let (window, button) =
            crate::test_support::open(cx, move |_, cx| PullButton::with_pull(id, pull, cx));
        cx.run_until_parked();
        (window, button)
    }

    fn shown(button: &Entity<PullButton>, cx: &mut TestAppContext) -> ToolbarButton {
        button.read_with(cx, |button, cx| button.button(cx))
    }

    #[gpui_kit::test]
    fn pulls_and_shows_how_many_commits_there_are_to_pull(cx: &mut TestAppContext) {
        let _dir = setup(cx);
        let repo = repository_id();
        set_status(&repo, status(), cx);
        let (pulls, pull) = pulls();
        let (window, button) = render(cx, &repo, pull);

        let shown = shown(&button, cx);
        assert_eq!(shown.tooltip(), "Pull 2 commits from origin/main");
        assert!(!shown.is_disabled());
        assert_eq!(shown.shown_count(), Some(2));

        cx.update_window(window, |_, window, cx| window.click("pull", cx))
            .unwrap();
        assert_eq!(*pulls.calls.borrow(), [repo.to_string()]);
    }

    #[gpui_kit::test]
    fn says_why_a_pull_failed(cx: &mut TestAppContext) {
        let _dir = setup(cx);
        let repo = repository_id();
        set_status(&repo, status(), cx);
        let (pulls, pull) = pulls();
        let (window, button) = render(cx, &repo, pull);

        cx.update_window(window, |_, window, cx| window.click("pull", cx))
            .unwrap();
        let settle = pulls.settle.borrow_mut().remove(0);
        settle
            .send(Err(QueryError::msg(
                "Pulling origin/main caused conflicts.",
            )))
            .unwrap();
        cx.run_until_parked();
        let error = shown(&button, cx).shown_failure().cloned();
        assert_eq!(
            error.map(|error| error.message()).as_deref(),
            Some("Pulling origin/main caused conflicts.")
        );

        cx.update_window(window, |_, window, cx| window.click("dismiss", cx))
            .unwrap();
        cx.run_until_parked();
        assert!(shown(&button, cx).shown_failure().is_none());
    }

    #[gpui_kit::test]
    fn is_disabled_without_an_upstream_to_pull_from(cx: &mut TestAppContext) {
        let _dir = setup(cx);
        let repo = repository_id();
        let mut summary = status();
        summary.summary.upstream = None;
        summary.summary.behind = 0;
        set_status(&repo, summary, cx);
        let (_, pull) = pulls();
        let (_, button) = render(cx, &repo, pull);

        let shown = shown(&button, cx);
        assert_eq!(shown.tooltip(), "main doesn't track a remote branch.");
        assert!(shown.is_disabled());
    }

    #[gpui_kit::test]
    fn keeps_a_pull_to_the_repository_it_was_started_in(cx: &mut TestAppContext) {
        let _dir = setup(cx);
        let repo = repository_id();
        let other = repository_id();
        set_status(&repo, status(), cx);
        set_status(&other, status(), cx);
        let (pulls, pull) = pulls();
        let (window, button) = render(cx, &repo, pull.clone());

        cx.update_window(window, |_, window, cx| window.click("pull", cx))
            .unwrap();
        assert!(shown(&button, cx).is_busy());

        // Another repository's pull button isn't busy with it…
        let (_, other_button) = render(cx, &other, pull.clone());
        let shown_other = shown(&other_button, cx);
        assert!(!shown_other.is_disabled());
        assert!(!shown_other.is_busy());

        // …and doesn't show why it failed.
        let settle = pulls.settle.borrow_mut().remove(0);
        settle
            .send(Err(QueryError::msg(
                "Pulling origin/main caused conflicts.",
            )))
            .unwrap();
        cx.run_until_parked();
        assert!(shown(&other_button, cx).shown_failure().is_none());

        // That repository's, made again, does.
        let (_, again) = render(cx, &repo, pull);
        assert!(shown(&again, cx).shown_failure().is_some());
    }

    #[gpui_kit::test]
    fn stays_busy_when_switching_away_from_a_running_pull_and_back(cx: &mut TestAppContext) {
        let _dir = setup(cx);
        let repo = repository_id();
        let other = repository_id();
        set_status(&repo, status(), cx);
        set_status(&other, status(), cx);
        let (pulls, pull) = pulls();
        let (window, _) = render(cx, &repo, pull.clone());

        cx.update_window(window, |_, window, cx| window.click("pull", cx))
            .unwrap();
        let _other = render(cx, &other, pull.clone());
        let (window, again) = render(cx, &repo, pull);
        let shown = shown(&again, cx);
        assert!(shown.is_busy());
        // A second click is ignored while it runs.
        cx.update_window(window, |_, window, cx| window.click("pull", cx))
            .unwrap();
        assert_eq!(pulls.calls.borrow().len(), 1);
    }
}
