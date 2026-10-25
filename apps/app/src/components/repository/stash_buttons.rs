//! The toolbar's Stash and Pop. Ported from `components/repository/stash-buttons.tsx`.

use gitto_git::input::RepositoryId;
use gitto_ui::git::stash::{can_pop, pop_title, stash_blocker, stash_title};
use gpui_kit::component::h_flex;
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Window};

use super::toolbar_button::{HideBelow, ToolbarButton};
use crate::components::ui::IconName;
use crate::data::changes::{POP, STASH};
use crate::data::{Part, Shown};

/// Stashes every change, untracked files included, and pops the newest stash; each says why if it
/// failed. One at a time, along with the stashes popped and deleted from the sidebar: a pop names
/// the stash it pops, which a stash still being made would change. An entity: it shows the status
/// and the stashes, and the actions' states, of the repository it's given.
pub struct StashButtons {
    repository: Shown,
}

impl StashButtons {
    pub fn new(repository_id: RepositoryId, _: &mut Window, cx: &mut Context<Self>) -> Self {
        StashButtons {
            repository: Shown::new(
                &repository_id,
                &[Part::Status, Part::Stashes, Part::Operations],
                cx,
            ),
        }
    }

    /// Shows another repository's.
    pub fn set_repository(&mut self, repository_id: RepositoryId, cx: &mut Context<Self>) {
        self.repository.set(&repository_id, cx);
    }

    /// Stash, as it is now. Until the status has loaded, it's disabled.
    pub fn stash_button(&self, cx: &App) -> ToolbarButton {
        let repository = self.repository.read(cx);
        let pending = repository.stashes_changing();
        let state = repository.operation_state(STASH);
        let status = repository.status().data();
        let title = status.map_or_else(
            || "Stash".to_owned(),
            |status| stash_title(&status.head, &status.counts),
        );
        let blocked =
            status.is_none_or(|status| stash_blocker(&status.head, &status.counts).is_some());
        let stash = self.repository.entity().clone();
        let dismiss = stash.clone();
        ToolbarButton::new("stash", IconName::Archive, "Stash")
            .hide_below(HideBelow::Large)
            .title(title)
            .disabled(blocked || pending)
            .busy(state.running)
            .failure("Stash", state.error, move |_, cx| {
                dismiss.update(cx, |repository, cx| repository.dismiss(STASH, cx))
            })
            .on_click(move |_, _, cx| {
                stash.update(cx, |repository, cx| drop(repository.stash(cx)));
            })
    }

    /// Pop, as it is now. Until the status and the stashes have loaded, it's disabled.
    pub fn pop_button(&self, cx: &App) -> ToolbarButton {
        let repository = self.repository.read(cx);
        let pending = repository.stashes_changing();
        let state = repository.operation_state(POP);
        let status = repository.status().data();
        let stashes = repository.stashes().data();
        let title = match (status, stashes, repository.stashes().error()) {
            (Some(status), Some(stashes), _) => pop_title(&status.counts, stashes),
            (_, None, Some(error)) => format!("Couldn't load the stashes: {}", error.message()),
            _ => "Pop".to_owned(),
        };
        let poppable = can_pop(status.map(|status| &status.counts), stashes.map(|s| &s[..]));
        let newest = stashes
            .and_then(|stashes| stashes.first())
            .map(|stash| stash.sha.clone());
        let pop = self.repository.entity().clone();
        let dismiss = pop.clone();
        ToolbarButton::new("pop", IconName::ArchiveRestore, "Pop")
            .hide_below(HideBelow::Large)
            .title(title)
            .disabled(!poppable || pending)
            .busy(state.running)
            .count(stashes.map(|stashes| stashes.len() as u64))
            .failure("Pop", state.error, move |_, cx| {
                dismiss.update(cx, |repository, cx| repository.dismiss(POP, cx))
            })
            .on_click(move |_, _, cx| {
                if let Some(newest) = newest.clone() {
                    pop.update(cx, |repository, cx| {
                        drop(repository.pop_stash(POP, newest, cx))
                    });
                }
            })
    }
}

impl Render for StashButtons {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_0p5()
            .child(self.stash_button(cx))
            .child(self.pop_button(cx))
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use futures::FutureExt as _;
    use futures::channel::oneshot;
    use gitto_git::input::FullSha;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AnyWindowHandle, Entity, TestAppContext};

    use super::*;
    use crate::components::repository::test_support::{
        changed, fake_git, repository_id, set_stashes, set_status, setup, stash, status,
    };
    use crate::data::DataError;
    use crate::data::git::{GitTask, RepositoryGit};

    /// What the buttons were asked to do, and what settles the stash under way.
    #[derive(Default)]
    struct Calls {
        stashes: RefCell<usize>,
        pops: RefCell<Vec<String>>,
        finish_stash: RefCell<Option<oneshot::Sender<Result<(), DataError>>>>,
        pop_answer: RefCell<Option<DataError>>,
    }

    impl RepositoryGit for Calls {
        fn push_stash(&self) -> GitTask<()> {
            *self.stashes.borrow_mut() += 1;
            let (finish, finished) = oneshot::channel();
            *self.finish_stash.borrow_mut() = Some(finish);
            async move { finished.await.unwrap_or(Ok(())) }.boxed()
        }

        fn pop_stash(&self, sha: FullSha) -> GitTask<()> {
            self.pops.borrow_mut().push(sha.as_str().to_owned());
            let answer = self.pop_answer.borrow_mut().take();
            async move { answer.map_or(Ok(()), Err) }.boxed()
        }
    }

    fn render(
        id: &RepositoryId,
        calls: &Rc<Calls>,
        cx: &mut TestAppContext,
    ) -> (AnyWindowHandle, Entity<StashButtons>) {
        fake_git(id, calls.clone(), cx);
        let shown = id.clone();
        crate::test_support::open(cx, move |window, cx| StashButtons::new(shown, window, cx))
    }

    fn click(window: AnyWindowHandle, id: &'static str, cx: &mut TestAppContext) {
        cx.update_window(window, |_, window, cx| window.click(id, cx))
            .unwrap();
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn stash_the_changes_and_pop_the_newest_stash_back(cx: &mut TestAppContext) {
        let _dir = setup(cx);
        let calls = Rc::new(Calls::default());
        let id = repository_id();
        set_status(&id, changed(status(), 2), cx);
        set_stashes(&id, vec![stash('a', "On main: older")], cx);
        let (window, buttons) = render(&id, &calls, cx);

        let pop = buttons.read_with(cx, |buttons, cx| buttons.pop_button(cx));
        assert_eq!(pop.tooltip(), "Pop \"On main: older\"");
        assert_eq!(pop.shown_count(), Some(1));
        let stash_button = buttons.read_with(cx, |buttons, cx| buttons.stash_button(cx));
        assert_eq!(stash_button.tooltip(), "Stash 2 changed files");

        click(window, "stash", cx);
        assert_eq!(*calls.stashes.borrow(), 1);
        assert!(buttons.read_with(cx, |buttons, cx| buttons.stash_button(cx).is_busy()));
        // A pop would name the stash that's being replaced as the newest.
        assert!(buttons.read_with(cx, |buttons, cx| buttons.pop_button(cx).is_disabled()));

        // The stash reloads the repository: clean, with the new stash first.
        set_status(&id, status(), cx);
        set_stashes(
            &id,
            vec![
                stash('b', "WIP on main: abc newer"),
                stash('a', "On main: older"),
            ],
            cx,
        );
        let finish = calls.finish_stash.borrow_mut().take().unwrap();
        finish.send(Ok(())).unwrap();
        cx.run_until_parked();
        let pop = buttons.read_with(cx, |buttons, cx| buttons.pop_button(cx));
        assert_eq!(pop.tooltip(), "Pop \"WIP on main: abc newer\"");
        assert_eq!(pop.shown_count(), Some(2));
        let stash_button = buttons.read_with(cx, |buttons, cx| buttons.stash_button(cx));
        assert_eq!(stash_button.tooltip(), "No changes to stash.");
        assert!(stash_button.is_disabled());

        click(window, "pop", cx);
        assert_eq!(*calls.pops.borrow(), ["b".repeat(40)]);
    }

    #[gpui_kit::test]
    fn say_why_a_pop_failed(cx: &mut TestAppContext) {
        let _dir = setup(cx);
        let calls = Rc::new(Calls::default());
        *calls.pop_answer.borrow_mut() =
            Some(DataError::msg("Popping the stash caused conflicts."));
        let (window, buttons) = {
            let id = repository_id();
            set_status(&id, changed(status(), 2), cx);
            set_stashes(&id, vec![stash('a', "On main: older")], cx);
            render(&id, &calls, cx)
        };

        click(window, "pop", cx);
        assert_eq!(*calls.pops.borrow(), ["a".repeat(40)]);
        let failure = buttons.read_with(cx, |buttons, cx| {
            buttons
                .pop_button(cx)
                .shown_failure()
                .map(DataError::message)
        });
        assert_eq!(
            failure.as_deref(),
            Some("Popping the stash caused conflicts.")
        );

        click(window, "dismiss", cx);
        let failure = buttons.read_with(cx, |buttons, cx| {
            buttons.pop_button(cx).shown_failure().cloned()
        });
        assert!(failure.is_none());
    }

    #[gpui_kit::test]
    fn are_disabled_with_nothing_to_stash_or_pop(cx: &mut TestAppContext) {
        let _dir = setup(cx);
        let calls = Rc::new(Calls::default());
        let id = repository_id();
        set_status(&id, status(), cx);
        set_stashes(&id, vec![], cx);
        let (_, buttons) = render(&id, &calls, cx);

        let stash_button = buttons.read_with(cx, |buttons, cx| buttons.stash_button(cx));
        assert_eq!(stash_button.tooltip(), "No changes to stash.");
        assert!(stash_button.is_disabled());
        let pop = buttons.read_with(cx, |buttons, cx| buttons.pop_button(cx));
        assert_eq!(pop.tooltip(), "No stashes to pop.");
        assert!(pop.is_disabled());
        assert_eq!(pop.shown_count(), None);
    }
}
