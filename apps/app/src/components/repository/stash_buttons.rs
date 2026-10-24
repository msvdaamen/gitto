//! The toolbar's Stash and Pop. Ported from `components/repository/stash-buttons.tsx`.

use std::sync::Arc;

use gitto_git::features::stash::schema::Stash;
use gitto_git::features::status::schema::{StatusSummary, Uncommitted};
use gitto_git::input::RepositoryId;
use gitto_ui::git::stash::{can_pop, pop_title, stash_blocker, stash_title};
use gpui_kit::component::h_flex;
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Subscription, Window};

use super::toolbar_button::{HideBelow, ToolbarButton};
use crate::components::ui::IconName;
use crate::data::stash::{StashActions, stash_actions, stashes_query};
use crate::data::status::status_query;
use crate::data::{Opaque, Operations, Query};

/// Stashes every change, untracked files included, and pops the newest stash; each says why if it
/// failed. One at a time, along with the stashes popped and deleted from the sidebar: a pop names
/// the stash it pops, which a stash still being made would change. An entity: it shows the status
/// and the stashes, and the actions' states, of the repository it's given.
pub struct StashButtons {
    repository_id: RepositoryId,
    status: Query<Opaque<Uncommitted>, StatusSummary>,
    stashes: Query<Arc<[Stash]>>,
    actions: StashActions,
    _subscriptions: Vec<Subscription>,
}

impl StashButtons {
    pub fn new(repository_id: RepositoryId, _: &mut Window, cx: &mut Context<Self>) -> Self {
        StashButtons::with_actions(repository_id, stash_actions(), cx)
    }

    /// With `actions` doing the stashing and popping: a test's.
    pub(crate) fn with_actions(
        repository_id: RepositoryId,
        actions: StashActions,
        cx: &mut Context<Self>,
    ) -> Self {
        StashButtons {
            status: status_query(&repository_id, cx),
            stashes: stashes_query(&repository_id, cx),
            repository_id,
            actions,
            _subscriptions: vec![cx.observe_global::<Operations>(|_, cx| cx.notify())],
        }
    }

    pub fn repository_id(&self) -> &RepositoryId {
        &self.repository_id
    }

    /// Shows another repository's.
    pub fn set_repository(&mut self, repository_id: RepositoryId, cx: &mut Context<Self>) {
        if self.repository_id != repository_id {
            self.status = status_query(&repository_id, cx);
            self.stashes = stashes_query(&repository_id, cx);
            self.repository_id = repository_id;
            cx.notify();
        }
    }

    /// Stash, as it is now. Until the status has loaded, it's disabled.
    pub fn stash_button(&self, cx: &App) -> ToolbarButton {
        let id = self.repository_id.clone();
        let pending = StashActions::is_pending(&id, cx);
        let state = self.actions.stash.state(&id, cx);
        let status = self.status.data();
        let title = status.as_ref().map_or_else(
            || "Stash".to_owned(),
            |status| stash_title(&status.head, &status.counts),
        );
        let blocked = status
            .as_ref()
            .is_none_or(|status| stash_blocker(&status.head, &status.counts).is_some());
        let stash = self.actions.stash.clone();
        let dismiss = self.actions.stash.clone();
        let dismissed = id.clone();
        ToolbarButton::new("stash", IconName::Archive, "Stash")
            .hide_below(HideBelow::Large)
            .title(title)
            .disabled(blocked || pending)
            .busy(state.running)
            .failure("Stash", state.error, move |_, cx| {
                dismiss.dismiss(&dismissed, cx)
            })
            .on_click(move |_, _, cx| {
                let _ = stash.run(&id, (), cx);
            })
    }

    /// Pop, as it is now. Until the status and the stashes have loaded, it's disabled.
    pub fn pop_button(&self, cx: &App) -> ToolbarButton {
        let id = self.repository_id.clone();
        let pending = StashActions::is_pending(&id, cx);
        let state = self.actions.pop.state(&id, cx);
        let status = self.status.data();
        let stashes = self.stashes.data();
        let title = match (&status, &stashes, self.stashes.error()) {
            (Some(status), Some(stashes), _) => pop_title(&status.counts, stashes),
            (_, None, Some(error)) => format!("Couldn't load the stashes: {}", error.message()),
            _ => "Pop".to_owned(),
        };
        let poppable = can_pop(
            status.as_ref().map(|status| &status.counts),
            stashes.as_deref(),
        );
        let newest = stashes
            .as_ref()
            .and_then(|stashes| stashes.first())
            .map(|stash| stash.sha.clone());
        let pop = self.actions.pop.clone();
        let dismiss = self.actions.pop.clone();
        let dismissed = id.clone();
        ToolbarButton::new("pop", IconName::ArchiveRestore, "Pop")
            .hide_below(HideBelow::Large)
            .title(title)
            .disabled(!poppable || pending)
            .busy(state.running)
            .count(stashes.map(|stashes| stashes.len() as u64))
            .failure("Pop", state.error, move |_, cx| {
                dismiss.dismiss(&dismissed, cx)
            })
            .on_click(move |_, _, cx| {
                if let Some(newest) = newest.clone() {
                    let _ = pop.run(&id, newest, cx);
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

    use futures::channel::oneshot;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AnyWindowHandle, AppContext as _, TestAppContext};

    use super::*;
    use crate::components::repository::test_support::{
        changed, repository_id, set_stashes, set_status, setup, stash, status,
    };
    use crate::data::{QueryError, RepositoryOperation};

    /// What the buttons were asked to do, and what settles the stash under way.
    #[derive(Default)]
    struct Calls {
        stashes: RefCell<usize>,
        pops: RefCell<Vec<String>>,
        finish_stash: RefCell<Option<oneshot::Sender<Result<(), QueryError>>>>,
        pop_answer: RefCell<Option<QueryError>>,
    }

    fn actions(calls: &Rc<Calls>) -> StashActions {
        let busy = |id: &str, cx: &App| StashActions::is_pending(id, cx);
        let unused = RepositoryOperation::new("unused", |_, _: String, cx: &mut App| {
            cx.background_spawn(async { Ok(()) })
        });
        StashActions {
            stash: RepositoryOperation::new("stash", {
                let calls = Rc::clone(calls);
                move |_, (), cx: &mut App| {
                    *calls.stashes.borrow_mut() += 1;
                    let (finish, finished) = oneshot::channel();
                    *calls.finish_stash.borrow_mut() = Some(finish);
                    cx.background_spawn(async move { finished.await.unwrap_or(Ok(())) })
                }
            })
            .one_at_a_time(busy),
            pop: RepositoryOperation::new("pop", {
                let calls = Rc::clone(calls);
                move |_, sha: String, cx: &mut App| {
                    calls.pops.borrow_mut().push(sha);
                    let answer = calls.pop_answer.borrow_mut().take();
                    cx.background_spawn(async move { answer.map_or(Ok(()), Err) })
                }
            })
            .one_at_a_time(busy),
            pop_picked: unused.clone().one_at_a_time(busy),
            drop: unused.one_at_a_time(busy),
        }
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
        let actions = actions(&calls);
        let shown = id.clone();
        let (window, buttons) = crate::test_support::open(cx, move |_, cx| {
            StashButtons::with_actions(shown, actions, cx)
        });

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
            Some(QueryError::msg("Popping the stash caused conflicts."));
        let (window, buttons) = {
            let id = repository_id();
            set_status(&id, changed(status(), 2), cx);
            set_stashes(&id, vec![stash('a', "On main: older")], cx);
            let actions = actions(&calls);
            let shown = id.clone();
            let (window, buttons) = crate::test_support::open(cx, move |_, cx| {
                StashButtons::with_actions(shown, actions, cx)
            });
            (window, buttons)
        };

        click(window, "pop", cx);
        assert_eq!(*calls.pops.borrow(), ["a".repeat(40)]);
        let failure = buttons.read_with(cx, |buttons, cx| {
            buttons
                .pop_button(cx)
                .shown_failure()
                .map(QueryError::message)
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
        let actions = actions(&calls);
        let shown = id.clone();
        let (_, buttons) = crate::test_support::open(cx, move |_, cx| {
            StashButtons::with_actions(shown, actions, cx)
        });

        let stash_button = buttons.read_with(cx, |buttons, cx| buttons.stash_button(cx));
        assert_eq!(stash_button.tooltip(), "No changes to stash.");
        assert!(stash_button.is_disabled());
        let pop = buttons.read_with(cx, |buttons, cx| buttons.pop_button(cx));
        assert_eq!(pop.tooltip(), "No stashes to pop.");
        assert!(pop.is_disabled());
        assert_eq!(pop.shown_count(), None);
    }
}
