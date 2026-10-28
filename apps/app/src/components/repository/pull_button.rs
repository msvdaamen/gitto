//! The toolbar's Pull. Ported from `components/repository/pull-button.tsx`.

use gitto_git::input::RepositoryId;
use gitto_ui_logic::git::status::{head_pull_blocker, pull_title};
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Window};

use super::toolbar_button::ToolbarButton;
use gitto_app_data::data::Change;
use gitto_app_data::data::{Part, Shown};
use gitto_app_kit::ui::IconName;

/// Pulls the current branch's upstream, and says why if that failed or was left to finish. An
/// entity: it shows the status (how many commits there are to pull, and why it can't) and the
/// pull's state, of the repository it's given. The pull is the repository's, wherever it was
/// started (a branch's menu pulls the checked-out one too): switching away and back shows it
/// still running, or why it failed.
pub struct PullButton {
    repository: Shown,
}

impl PullButton {
    pub fn new(repository_id: RepositoryId, _: &mut Window, cx: &mut Context<Self>) -> Self {
        PullButton {
            repository: Shown::new(&repository_id, &[Part::Status, Part::Changes], cx),
        }
    }

    /// Shows another repository's.
    pub fn set_repository(&mut self, repository_id: RepositoryId, cx: &mut Context<Self>) {
        self.repository.set(&repository_id, cx);
    }

    /// The button as it is now. Until the status has loaded, it's disabled.
    pub fn button(&self, cx: &App) -> ToolbarButton {
        let repository = self.repository.read(cx);
        let state = repository.change_state(Change::Pull);
        let status = repository.status().data();
        let title = status.map_or_else(
            || "Pull".to_owned(),
            |status| pull_title(&status.head, status.upstream.as_deref(), status.behind),
        );
        let blocked = status.is_none_or(|status| {
            head_pull_blocker(&status.head, status.upstream.as_deref()).is_some()
        });
        let pull = self.repository.entity().clone();
        let dismiss = pull.clone();
        ToolbarButton::new("pull", IconName::Download, "Pull")
            .accent(true)
            .title(title)
            .disabled(blocked || state.running)
            .busy(state.running)
            .count(status.map(|status| status.behind))
            .failure("Pull", state.error, move |_, cx| {
                dismiss.update(cx, |repository, cx| repository.dismiss(Change::Pull, cx))
            })
            .on_click(move |_, _, cx| {
                pull.update(cx, |repository, cx| drop(repository.pull(cx)));
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

    use futures::FutureExt as _;
    use futures::channel::oneshot;
    use gitto_git::input::NonEmpty;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AnyWindowHandle, Entity, TestAppContext};

    use super::*;
    use crate::components::repository::test_support::{
        fake_git, repository_id, set_status, setup, status,
    };
    use gitto_app_data::data::DataError;
    use gitto_app_data::data::git::{GitTask, RepositoryGit};

    /// A repository's git whose pulls the test settles.
    #[derive(Default)]
    struct Pulls {
        settle: RefCell<Vec<oneshot::Sender<Result<(), DataError>>>>,
    }

    impl RepositoryGit for Pulls {
        fn pull(&self, _: Option<NonEmpty>) -> GitTask<()> {
            let (settle, settled) = oneshot::channel();
            self.settle.borrow_mut().push(settle);
            async move { settled.await.unwrap_or(Err(DataError::msg("dropped"))) }.boxed()
        }
    }

    fn pulls(id: &RepositoryId, cx: &mut TestAppContext) -> Rc<Pulls> {
        let pulls = Rc::new(Pulls::default());
        fake_git(id, pulls.clone(), cx);
        pulls
    }

    fn render(cx: &mut TestAppContext, id: &RepositoryId) -> (AnyWindowHandle, Entity<PullButton>) {
        let id = id.clone();
        let (window, button) =
            crate::test_support::open(cx, move |window, cx| PullButton::new(id, window, cx));
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
        let pulls = pulls(&repo, cx);
        let (window, button) = render(cx, &repo);

        let shown = shown(&button, cx);
        assert_eq!(shown.tooltip(), "Pull 2 commits from origin/main");
        assert!(!shown.is_disabled());
        assert_eq!(shown.shown_count(), Some(2));

        cx.update_window(window, |_, window, cx| window.click("pull", cx))
            .unwrap();
        cx.run_until_parked();
        assert_eq!(pulls.settle.borrow().len(), 1);
    }

    #[gpui_kit::test]
    fn says_why_a_pull_failed(cx: &mut TestAppContext) {
        let _dir = setup(cx);
        let repo = repository_id();
        set_status(&repo, status(), cx);
        let pulls = pulls(&repo, cx);
        let (window, button) = render(cx, &repo);

        cx.update_window(window, |_, window, cx| window.click("pull", cx))
            .unwrap();
        cx.run_until_parked();
        let settle = pulls.settle.borrow_mut().remove(0);
        settle
            .send(Err(DataError::msg("Pulling origin/main caused conflicts.")))
            .unwrap();
        cx.run_until_parked();
        let error = shown(&button, cx).shown_failure().cloned();
        assert_eq!(
            error.map(|error| error.to_string()).as_deref(),
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
        summary.upstream = None;
        summary.behind = 0;
        set_status(&repo, summary, cx);
        let (_, button) = render(cx, &repo);

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
        let pulls = pulls(&repo, cx);
        let (window, button) = render(cx, &repo);

        cx.update_window(window, |_, window, cx| window.click("pull", cx))
            .unwrap();
        cx.run_until_parked();
        assert!(shown(&button, cx).is_busy());

        // Another repository's pull button isn't busy with it…
        button.update(cx, |button, cx| button.set_repository(other.clone(), cx));
        let shown_other = shown(&button, cx);
        assert!(!shown_other.is_disabled());
        assert!(!shown_other.is_busy());

        // …and doesn't show why it failed.
        let settle = pulls.settle.borrow_mut().remove(0);
        settle
            .send(Err(DataError::msg("Pulling origin/main caused conflicts.")))
            .unwrap();
        cx.run_until_parked();
        assert!(shown(&button, cx).shown_failure().is_none());

        // Back in that repository, it does.
        button.update(cx, |button, cx| button.set_repository(repo.clone(), cx));
        assert!(shown(&button, cx).shown_failure().is_some());
    }

    #[gpui_kit::test]
    fn stays_busy_when_switching_away_from_a_running_pull_and_back(cx: &mut TestAppContext) {
        let _dir = setup(cx);
        let repo = repository_id();
        let other = repository_id();
        set_status(&repo, status(), cx);
        set_status(&other, status(), cx);
        let pulls = pulls(&repo, cx);
        let (window, button) = render(cx, &repo);

        cx.update_window(window, |_, window, cx| window.click("pull", cx))
            .unwrap();
        cx.run_until_parked();
        button.update(cx, |button, cx| button.set_repository(other.clone(), cx));
        button.update(cx, |button, cx| button.set_repository(repo.clone(), cx));
        let shown = shown(&button, cx);
        assert!(shown.is_busy());
        // A second click is ignored while it runs.
        cx.update_window(window, |_, window, cx| window.click("pull", cx))
            .unwrap();
        cx.run_until_parked();
        assert_eq!(pulls.settle.borrow().len(), 1);
    }
}
