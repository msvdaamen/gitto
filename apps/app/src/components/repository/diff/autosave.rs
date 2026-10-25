//! Saving a file's edits a moment after each. Ported from `autosave.ts`.

use std::rc::Rc;
use std::time::Duration;

use futures::FutureExt as _;
use futures::future::Shared;
use gitto_git::ApiCode;
use gpui_kit::{App, Context, Task};

use crate::data::QueryError;

/// Where saving a file that's being edited is at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveState {
    Saved,
    /// Edited since the last save, which is due once the typing stops.
    Pending,
    Saving,
    /// The file changed on disk since it was read: not saved until the user says what to keep.
    ChangedOnDisk(String),
    /// Saving failed otherwise, e.g. the disk is full; tried again with the next edit.
    Failed(String),
}

impl SaveState {
    /// Why the edits aren't saved, if saving failed.
    pub fn message(&self) -> Option<&str> {
        match self {
            SaveState::ChangedOnDisk(message) | SaveState::Failed(message) => Some(message),
            _ => None,
        }
    }
}

/// Saves `text` over the file as it was at `version`, or whatever it is if `overwrite`; resolves
/// to the version saved.
pub type SaveFile =
    Rc<dyn Fn(String, String, bool, &mut App) -> Task<Result<String, QueryError>> + 'static>;

/// How long after the last edit it's saved: long enough not to write on every key.
pub const AUTOSAVE_DELAY: Duration = Duration::from_millis(600);

/// Saves a file's edits a moment after each, one save at a time, each over the version the last
/// one wrote: one that changed on disk since isn't saved over unless the user says so
/// ([`overwrite`](Self::overwrite)). Started with the version the edits are of
/// ([`start`](Self::start)); [`flush`](Self::flush) saves what's left at once, e.g. before the file
/// is closed. An entity, which its owner observes for its [`state`](Self::state).
pub struct Autosave {
    save: SaveFile,
    delay: Duration,
    state: SaveState,
    version: Option<String>,
    /// The edits to save; `None` once they're saved.
    unsaved: Option<String>,
    /// Counts the timers set: one that fires after another was set, or cleared, does nothing.
    timer: u64,
    /// The saves asked for, one after the other: the last one.
    saves: Shared<Task<()>>,
}

impl Autosave {
    pub fn new(save: SaveFile, delay: Duration) -> Self {
        Autosave {
            save,
            delay,
            state: SaveState::Saved,
            version: None,
            unsaved: None,
            timer: 0,
            saves: Task::ready(()).shared(),
        }
    }

    pub fn state(&self) -> &SaveState {
        &self.state
    }

    /// Whether there are edits that aren't saved.
    pub fn is_dirty(&self) -> bool {
        self.unsaved.is_some()
    }

    fn set_state(&mut self, state: SaveState, cx: &mut Context<Self>) {
        if self.state != state {
            self.state = state;
            cx.notify();
        }
    }

    /// Saves over the file as it is at `version` from now on, with nothing to save yet.
    pub fn start(&mut self, version: String, cx: &mut Context<Self>) {
        self.clear_timer();
        self.version = Some(version);
        self.unsaved = None;
        self.set_state(SaveState::Saved, cx);
    }

    /// The file was edited to `text`: saved once the edits stop for a moment.
    pub fn change(&mut self, text: String, cx: &mut Context<Self>) {
        self.unsaved = Some(text);
        if matches!(self.state, SaveState::ChangedOnDisk(_)) {
            return;
        }
        self.set_state(SaveState::Pending, cx);
        self.schedule(cx);
    }

    /// Saves the edits that are left now; resolves to whether everything's saved.
    pub fn flush(&mut self, cx: &mut Context<Self>) -> Task<bool> {
        let waiting = if self.unsaved.is_none() {
            self.saves.clone()
        } else {
            self.run(false, cx)
        };
        self.saved_after(waiting, cx)
    }

    /// Sends the edits that are left at once, as the app quits: not after a save under way, whose
    /// answer may come too late. Over the version that save started from, which the save takes as
    /// the one it's saving (see `save_working_tree_file`).
    pub fn flush_now(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<Task<Result<String, QueryError>>> {
        self.clear_timer();
        let (text, version) = (self.unsaved.clone()?, self.version.clone()?);
        if matches!(self.state, SaveState::ChangedOnDisk(_)) {
            return None;
        }
        Some((self.save)(text, version, false, cx))
    }

    /// Saves the edits over the file whatever it is now; resolves to whether they're saved.
    pub fn overwrite(&mut self, cx: &mut Context<Self>) -> Task<bool> {
        let waiting = self.run(true, cx);
        self.saved_after(waiting, cx)
    }

    /// Forgets the edits that aren't saved, and stops saving.
    pub fn stop(&mut self, cx: &mut Context<Self>) {
        self.clear_timer();
        self.unsaved = None;
        self.version = None;
        self.set_state(SaveState::Saved, cx);
    }

    /// Whether everything's saved, once `waiting` is done.
    fn saved_after(&self, waiting: Shared<Task<()>>, cx: &mut Context<Self>) -> Task<bool> {
        cx.spawn(async move |this, cx| {
            waiting.await;
            this.read_with(cx, |this, _| {
                this.unsaved.is_none() && this.state == SaveState::Saved
            })
            .unwrap_or(false)
        })
    }

    fn clear_timer(&mut self) {
        self.timer += 1;
    }

    fn schedule(&mut self, cx: &mut Context<Self>) {
        self.clear_timer();
        let timer = self.timer;
        let delay = self.delay;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update(cx, |this, cx| {
                if this.timer == timer {
                    drop(this.run(false, cx));
                }
            });
        })
        .detach();
    }

    /// Saves the latest edits, after any save under way.
    fn run(&mut self, overwrite: bool, cx: &mut Context<Self>) -> Shared<Task<()>> {
        self.clear_timer();
        let previous = self.saves.clone();
        let saves = cx
            .spawn(async move |this, cx| {
                previous.await;
                let Ok(Some((text, saving))) =
                    this.update(cx, |this, cx| this.save_latest(overwrite, cx))
                else {
                    return;
                };
                let result = saving.await;
                let _ = this.update(cx, |this, cx| this.saved(text, result, cx));
            })
            .shared();
        self.saves = saves.clone();
        saves
    }

    /// Starts saving the latest edits, if there are any to save.
    fn save_latest(
        &mut self,
        overwrite: bool,
        cx: &mut Context<Self>,
    ) -> Option<(String, Task<Result<String, QueryError>>)> {
        let (text, version) = (self.unsaved.clone()?, self.version.clone()?);
        if !overwrite && matches!(self.state, SaveState::ChangedOnDisk(_)) {
            return None;
        }
        self.set_state(SaveState::Saving, cx);
        let saving = (self.save)(text.clone(), version, overwrite, cx);
        Some((text, saving))
    }

    /// `text` was saved, or not.
    fn saved(&mut self, text: String, result: Result<String, QueryError>, cx: &mut Context<Self>) {
        // Stopped meanwhile: nothing left to save, nor to say about it.
        if self.version.is_none() {
            return;
        }
        match result {
            Err(error) => {
                let message = error.message();
                let state = if is_changed_on_disk(&error) {
                    SaveState::ChangedOnDisk(message)
                } else {
                    SaveState::Failed(message)
                };
                self.set_state(state, cx);
            }
            Ok(version) => {
                self.version = Some(version);
                if self.unsaved.as_deref() == Some(&text) {
                    self.unsaved = None;
                    self.set_state(SaveState::Saved, cx);
                } else if self.unsaved.is_some() {
                    // Edited again meanwhile.
                    self.set_state(SaveState::Pending, cx);
                    self.schedule(cx);
                } else {
                    self.set_state(SaveState::Saved, cx);
                }
            }
        }
    }
}

/// Whether saving failed because the file changed, or went, on disk since it was read: git's API
/// error for that (`Error::FileChangedOnDisk`, a `CONFLICT`), which an overwrite doesn't get.
fn is_changed_on_disk(error: &QueryError) -> bool {
    error.has_code(ApiCode::Conflict)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use futures::channel::oneshot;
    use gitto_git::core::errors::DiskChange;
    use gitto_git::{ApiError, Error};
    use gpui_kit::{AppContext as _, Entity, TestAppContext};

    use super::*;

    const DELAY: Duration = Duration::from_millis(100);

    /// The saves asked for: text, version, and whether to overwrite.
    type Calls = Rc<RefCell<Vec<(String, String, bool)>>>;

    fn autosave(
        cx: &mut TestAppContext,
        respond: impl Fn(&str, &str, bool, &mut App) -> Task<Result<String, QueryError>> + 'static,
    ) -> (Entity<Autosave>, Calls) {
        let calls: Calls = Rc::default();
        let recorded = Rc::clone(&calls);
        let save: SaveFile = Rc::new(move |text, version, overwrite, cx| {
            recorded
                .borrow_mut()
                .push((text.clone(), version.clone(), overwrite));
            respond(&text, &version, overwrite, cx)
        });
        (cx.new(|_| Autosave::new(save, DELAY)), calls)
    }

    fn call(text: &str, version: &str, overwrite: bool) -> (String, String, bool) {
        (text.into(), version.into(), overwrite)
    }

    /// An error as git's API makes it for a file changed on disk.
    fn changed_on_disk() -> QueryError {
        ApiError::from(Error::FileChangedOnDisk {
            path: "a.txt".into(),
            how: DiskChange::Changed,
        })
        .into()
    }

    fn state(autosave: &Entity<Autosave>, cx: &mut TestAppContext) -> SaveState {
        autosave.read_with(cx, |autosave, _| autosave.state().clone())
    }

    fn wait(cx: &mut TestAppContext, by: Duration) {
        cx.executor().advance_clock(by);
        cx.run_until_parked();
    }

    fn resolved(task: Task<bool>, cx: &mut TestAppContext) -> bool {
        let result = Rc::new(RefCell::new(None));
        let done = Rc::clone(&result);
        cx.update(|cx| {
            cx.spawn(async move |_| *done.borrow_mut() = Some(task.await))
                .detach()
        });
        cx.run_until_parked();
        result.borrow_mut().take().expect("resolved")
    }

    #[gpui_kit::test]
    fn saves_once_the_edits_stop_over_the_version_the_last_save_wrote(cx: &mut TestAppContext) {
        let (autosave, calls) = autosave(cx, |text, _, _, _| Task::ready(Ok(format!("v-{text}"))));
        autosave.update(cx, |autosave, cx| autosave.start("v0".into(), cx));

        autosave.update(cx, |autosave, cx| {
            autosave.change("a".into(), cx);
            autosave.change("ab".into(), cx);
        });
        assert_eq!(state(&autosave, cx), SaveState::Pending);
        wait(cx, DELAY);
        assert_eq!(*calls.borrow(), [call("ab", "v0", false)]);
        assert_eq!(state(&autosave, cx), SaveState::Saved);

        autosave.update(cx, |autosave, cx| autosave.change("abc".into(), cx));
        wait(cx, DELAY);
        assert_eq!(calls.borrow().last(), Some(&call("abc", "v-ab", false)));
    }

    #[gpui_kit::test]
    fn saves_whats_left_at_once_when_flushed(cx: &mut TestAppContext) {
        let (autosave, calls) = autosave(cx, |_, _, _, _| Task::ready(Ok("v1".into())));
        autosave.update(cx, |autosave, cx| {
            autosave.start("v0".into(), cx);
            autosave.change("a".into(), cx);
        });

        let flushed = autosave.update(cx, |autosave, cx| autosave.flush(cx));
        assert!(resolved(flushed, cx));
        assert_eq!(calls.borrow().len(), 1);
        assert!(!autosave.read_with(cx, |autosave, _| autosave.is_dirty()));
    }

    #[gpui_kit::test]
    fn holds_the_edits_once_the_file_changed_on_disk_until_its_overwritten(
        cx: &mut TestAppContext,
    ) {
        let (autosave, calls) = autosave(cx, |_, _, overwrite, _| {
            Task::ready(if overwrite {
                Ok("v1".into())
            } else {
                Err(changed_on_disk())
            })
        });
        autosave.update(cx, |autosave, cx| {
            autosave.start("v0".into(), cx);
            autosave.change("a".into(), cx);
        });

        let flushed = autosave.update(cx, |autosave, cx| autosave.flush(cx));
        assert!(!resolved(flushed, cx));
        assert!(matches!(state(&autosave, cx), SaveState::ChangedOnDisk(_)));
        // Further edits aren't saved over it either.
        autosave.update(cx, |autosave, cx| autosave.change("ab".into(), cx));
        wait(cx, Duration::from_millis(500));
        assert_eq!(calls.borrow().len(), 1);

        let overwritten = autosave.update(cx, |autosave, cx| autosave.overwrite(cx));
        assert!(resolved(overwritten, cx));
        assert_eq!(calls.borrow().last(), Some(&call("ab", "v0", true)));
    }

    #[gpui_kit::test]
    fn tries_a_failed_save_again_with_the_next_edit(cx: &mut TestAppContext) {
        let failed = Rc::new(RefCell::new(false));
        let (autosave, _) = autosave(cx, move |_, _, _, _| {
            let first = !failed.replace(true);
            Task::ready(if first {
                Err(QueryError::msg("The disk is full."))
            } else {
                Ok("v1".into())
            })
        });
        autosave.update(cx, |autosave, cx| {
            autosave.start("v0".into(), cx);
            autosave.change("a".into(), cx);
        });
        wait(cx, DELAY);
        assert_eq!(
            state(&autosave, cx),
            SaveState::Failed("The disk is full.".into())
        );

        autosave.update(cx, |autosave, cx| autosave.change("ab".into(), cx));
        wait(cx, DELAY);
        assert_eq!(state(&autosave, cx), SaveState::Saved);
    }

    #[gpui_kit::test]
    fn saves_edits_made_while_a_save_is_under_way_after_it(cx: &mut TestAppContext) {
        let finish = Rc::new(RefCell::new(None::<oneshot::Sender<String>>));
        let pending = Rc::clone(&finish);
        let (autosave, calls) = autosave(cx, move |_, _, _, cx| {
            if pending.borrow().is_some() {
                return Task::ready(Ok("v2".into()));
            }
            let (send, receive) = oneshot::channel::<String>();
            *pending.borrow_mut() = Some(send);
            cx.spawn(async move |_| receive.await.map_err(|_| QueryError::msg("Dropped.")))
        });
        autosave.update(cx, |autosave, cx| {
            autosave.start("v0".into(), cx);
            autosave.change("a".into(), cx);
        });
        wait(cx, DELAY);
        autosave.update(cx, |autosave, cx| autosave.change("ab".into(), cx));
        let send = finish.borrow_mut().take().expect("a save under way");
        // Taken: the next save resolves at once.
        *finish.borrow_mut() = Some(oneshot::channel().0);
        send.send("v1".into()).unwrap();
        wait(cx, DELAY);
        assert_eq!(calls.borrow().last(), Some(&call("ab", "v1", false)));
        assert_eq!(state(&autosave, cx), SaveState::Saved);
    }

    #[gpui_kit::test]
    fn sends_whats_left_at_once_as_the_window_closes_over_the_version_a_save_under_way_started_from(
        cx: &mut TestAppContext,
    ) {
        let first = Rc::new(RefCell::new(true));
        let (autosave, calls) = autosave(cx, move |_, _, _, cx| {
            if first.replace(false) {
                // Never answered.
                return cx.spawn(async move |_| futures::future::pending().await);
            }
            Task::ready(Ok("v2".into()))
        });
        autosave.update(cx, |autosave, cx| {
            autosave.start("v0".into(), cx);
            autosave.change("a".into(), cx);
        });
        wait(cx, DELAY);
        autosave.update(cx, |autosave, cx| autosave.change("ab".into(), cx));

        let sent = autosave.update(cx, |autosave, cx| autosave.flush_now(cx));
        assert!(sent.is_some());
        assert_eq!(calls.borrow().last(), Some(&call("ab", "v0", false)));
    }

    #[gpui_kit::test]
    fn says_nothing_of_a_save_that_failed_after_it_was_stopped(cx: &mut TestAppContext) {
        let finish = Rc::new(RefCell::new(None::<oneshot::Sender<String>>));
        let pending = Rc::clone(&finish);
        let (autosave, _) = autosave(cx, move |_, _, _, cx| {
            let (send, receive) = oneshot::channel::<String>();
            *pending.borrow_mut() = Some(send);
            cx.spawn(async move |_| {
                receive.await.ok();
                Err(changed_on_disk())
            })
        });
        autosave.update(cx, |autosave, cx| {
            autosave.start("v0".into(), cx);
            autosave.change("a".into(), cx);
        });
        wait(cx, DELAY);
        assert_eq!(state(&autosave, cx), SaveState::Saving);
        // The edits are dropped while they're being saved, which then fails.
        autosave.update(cx, |autosave, cx| autosave.stop(cx));
        drop(finish.borrow_mut().take());
        cx.run_until_parked();
        assert_eq!(state(&autosave, cx), SaveState::Saved);
    }

    #[gpui_kit::test]
    fn takes_a_file_thats_gone_or_a_repository_for_a_failed_save_not_one_to_overwrite(
        cx: &mut TestAppContext,
    ) {
        let (autosave, _) = autosave(cx, |_, _, _, _| {
            Task::ready(Err(ApiError::from(Error::RepositoryNotFound {
                repository_id: "repo".into(),
            })
            .into()))
        });
        autosave.update(cx, |autosave, cx| {
            autosave.start("v0".into(), cx);
            autosave.change("a".into(), cx);
        });
        let flushed = autosave.update(cx, |autosave, cx| autosave.flush(cx));
        assert!(!resolved(flushed, cx));
        assert!(matches!(state(&autosave, cx), SaveState::Failed(_)));
    }
}
