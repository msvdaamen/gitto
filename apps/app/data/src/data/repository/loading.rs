//! Loading what a [`Repository`] shows: each field, again when it changes, and more of the log.

use std::rc::Rc;

use futures::FutureExt as _;
use futures::channel::oneshot;
use gitto_git::features::status::schema::StatusReply;
use gpui_kit::{AppContext as _, Context};

use super::{Changed, Field, Part, PatchKey, Reloading, Repository, log_commits};
use crate::data::error::DataError;
use crate::data::git::GitTask;
use crate::data::history::{NEXT_PAGE, followed_by, reload_log};
use crate::data::loaded::{LoadState, Loaded};
use crate::data::opaque::Opaque;
use gitto_ui_logic::git::diff_source::is_uncommitted;

impl Repository {
    /// Loads `field` unless it's loaded, or loading: for a view about to show it.
    pub fn show(&mut self, field: Field, cx: &mut Context<Self>) {
        let state = self.state(field);
        if state.wants_load() {
            drop(self.reload(field, cx));
        }
    }

    /// Where `field` is in loading, whatever it holds.
    fn state(&self, field: Field) -> &dyn LoadState {
        match field {
            Field::Status => &self.status,
            Field::Operation => &self.operation,
            Field::Log => &self.log,
            Field::Refs => &self.refs,
            Field::Remotes => &self.remotes,
            Field::Stashes => &self.stashes,
            Field::Worktrees => &self.worktrees,
            Field::PushTarget => &self.push_target,
            Field::Overview => &self.overview,
        }
    }

    /// Loads `field` again now, in place of (and so cancelling) a load under way; resolves once
    /// it's done.
    pub fn reload(&mut self, field: Field, cx: &mut Context<Self>) -> Reloading {
        let git = Rc::clone(&self.git);
        let part = field.part();
        let wait = match field {
            Field::Status => {
                let previous = self.status.data().cloned();
                let since = previous.as_ref().map(|status| status.version.clone());
                let reply = git.status(since);
                self.start(
                    part,
                    |repo| Some(&mut repo.status),
                    async move {
                        match reply.await? {
                            // Only ever the answer when there was a previous status to compare
                            // with.
                            StatusReply::Unchanged { .. } => previous.ok_or_else(|| {
                                DataError::msg("The status was unchanged, from none.")
                            }),
                            StatusReply::Status(status) => Ok(Opaque::new(*status)),
                        }
                    }
                    .boxed(),
                    cx,
                )
            }
            Field::Operation => {
                self.start(part, |repo| Some(&mut repo.operation), git.operation(), cx)
            }
            Field::Log => {
                let loaded = self.log.data().map_or(0, |log| log.commits.len());
                let fetch = reload_log(&*git, &self.id, loaded);
                // Loading more of the log as it was would leave it out of order.
                if self.more.task.take().is_some() || self.more.error.take().is_some() {
                    cx.emit(Changed(Part::More));
                }
                self.start(part, |repo| Some(&mut repo.log), fetch, cx)
            }
            Field::Refs => {
                let refs = git.refs();
                let fetch = async move { Ok(refs.await?.into()) }.boxed();
                self.start(part, |repo| Some(&mut repo.refs), fetch, cx)
            }
            Field::Remotes => self.start(part, |repo| Some(&mut repo.remotes), git.remotes(), cx),
            Field::Stashes => {
                let stashes = git.stashes();
                let fetch = async move { Ok(stashes.await?.into()) }.boxed();
                self.start(part, |repo| Some(&mut repo.stashes), fetch, cx)
            }
            Field::Worktrees => {
                let worktrees = git.worktrees();
                let fetch = async move { Ok(worktrees.await?.into()) }.boxed();
                self.start(part, |repo| Some(&mut repo.worktrees), fetch, cx)
            }
            Field::PushTarget => self.start(
                part,
                |repo| Some(&mut repo.push_target),
                git.push_target(),
                cx,
            ),
            Field::Overview => {
                self.start(part, |repo| Some(&mut repo.overview), git.overview(), cx)
            }
        };
        Reloading(wait.into_iter().collect())
    }

    /// Loads, with `fetch`, what `at` finds, in place of a load under way; a receiver told once
    /// it's done. Says it `Changed` as it starts loading, unless it was loading already, and as
    /// it's done.
    pub(super) fn start<T: PartialEq + Send + 'static>(
        &mut self,
        part: Part,
        at: impl Fn(&mut Self) -> Option<&mut Loaded<T>> + 'static,
        fetch: GitTask<T>,
        cx: &mut Context<Self>,
    ) -> Option<oneshot::Receiver<()>> {
        let at = Rc::new(at);
        let fetch = cx.background_spawn(fetch);
        let settle = Rc::clone(&at);
        let task = cx.spawn(async move |this, cx| {
            let result = fetch.await;
            let _ = this.update(cx, |repo, cx| {
                if let Some(loaded) = settle(repo) {
                    loaded.settle(result);
                    cx.emit(Changed(part));
                }
            });
        });
        let loaded = at(self)?;
        let was_loading = loaded.is_fetching();
        loaded.start(task);
        let wait = loaded.wait();
        if !was_loading {
            cx.emit(Changed(part));
        }
        wait
    }

    /// Loads again what's shown of the repository, after it changed on disk, or a change to it (a
    /// pull, say); what nothing shows is loaded again once something does. What the home page
    /// shows of it (the overview) is only loaded there, when it's shown.
    pub fn reload_all(&mut self, cx: &mut Context<Self>) -> Reloading {
        let mut waits = self.reload_uncommitted(cx).0;
        for field in Field::ALL {
            match field {
                // Reloaded with the uncommitted changes.
                Field::Status => {}
                // Loaded where it's shown (see above).
                Field::Overview => self.overview.mark_stale(),
                _ if self.state(field).is_used() => waits.extend(self.reload(field, cx).0),
                _ => {}
            }
        }
        for sha in self.pushed_to.held_and_drop_rest(|_| true) {
            waits.extend(self.load_pushed_to(sha, cx));
        }
        Reloading(waits)
    }

    /// Loads again what's shown of the uncommitted changes, after a file changed, or the index: the
    /// status, and the patches and conflicted files on show.
    pub fn reload_uncommitted(&mut self, cx: &mut Context<Self>) -> Reloading {
        self.reload_uncommitted_but(None, cx)
    }

    /// [`reload_uncommitted`](Self::reload_uncommitted), but for the patch `kept`, which is up to
    /// date: one a change just told.
    pub(crate) fn reload_uncommitted_but(
        &mut self,
        kept: Option<PatchKey>,
        cx: &mut Context<Self>,
    ) -> Reloading {
        let mut waits = Vec::new();
        if self.status.is_used() {
            waits.extend(self.reload(Field::Status, cx).0);
        }
        let patches = self.patches.held_and_drop_rest(|key| {
            is_uncommitted(&key.source) && kept.as_ref().is_none_or(|kept| kept != key)
        });
        for key in patches {
            let file = self
                .patches
                .get(&key)
                .and_then(Loaded::data)
                .map(|patch| patch.file.clone());
            if let Some(file) = file {
                waits.extend(self.load_patch(key.source, file, cx));
            }
        }
        for path in self.conflicts.held_and_drop_rest(|_| true) {
            waits.extend(self.load_conflict(path, cx));
        }
        cx.emit(Changed(Part::Patches));
        Reloading(waits)
    }

    /// Loads again what was loaded before the repository came on show, when nothing watched it,
    /// and may be out of date; not what's loading, which is loaded since.
    pub fn reload_when_shown(&mut self, cx: &mut Context<Self>) {
        for field in Field::ALL {
            if field == Field::Overview {
                self.overview.mark_stale();
                continue;
            }
            let state = self.state(field);
            if state.is_used() && !state.is_fetching() {
                drop(self.reload(field, cx));
            }
        }
        drop(self.pushed_to.held_and_drop_rest(|_| true));
        drop(
            self.patches
                .held_and_drop_rest(|key| is_uncommitted(&key.source)),
        );
        drop(self.conflicts.held_and_drop_rest(|_| true));
    }

    /// Drops what's kept by a key that nothing shows, as the repository went off show (see
    /// `RepositoryWatcher`): each repository opened kept some of each, for the rest of the
    /// session. What's shown again is loaded again.
    pub fn went_off_show(&mut self) {
        self.pushed_to.drop_unheld();
        self.patches.drop_unheld();
        self.conflicts.drop_unheld();
        self.blobs.drop_unheld();
        self.commits.drop_unheld();
        self.files.drop_unheld();
        self.messages.drop_unheld();
    }

    // Loading more of the log.

    /// Whether more of the log is loading.
    pub fn is_loading_more(&self) -> bool {
        self.more.task.is_some()
    }

    /// Why loading more of the log last failed, if it did, since it last reloaded.
    pub fn more_error(&self) -> Option<&DataError> {
        self.more.error.as_ref()
    }

    /// Whether there's more of the log to load, once it's loaded.
    pub fn has_more(&self) -> bool {
        self.log.data().is_some_and(|log| !log.complete)
    }

    /// Loads `NEXT_PAGE` more commits of the log, after those loaded, unless it's all loaded, or
    /// loading, or reloading, which would leave that unused.
    pub fn load_more(&mut self, cx: &mut Context<Self>) {
        if !self.has_more() || self.log.is_fetching() || self.is_loading_more() {
            return;
        }
        let Some(log) = self.log.data().cloned() else {
            return;
        };
        let page =
            cx.background_spawn(log_commits(&*self.git, log.commits.len() as u32, NEXT_PAGE));
        self.more.error = None;
        self.more.task = Some(cx.spawn(async move |this, cx| {
            let result = page.await;
            let _ = this.update(cx, |repo, cx| {
                // A reload since dropped this task, so this follows on from what's there.
                repo.more.task = None;
                match result {
                    Ok(commits) => {
                        let complete = commits.len() < NEXT_PAGE as usize;
                        repo.log.set(followed_by(&log, commits, complete));
                        cx.emit(Changed(Part::Log));
                    }
                    // The pages before it were read unsorted, and turned out to need sorting:
                    // the log's read again from the top, as many commits as were there, rather
                    // than skipping on from them.
                    Err(error) if is_log_order_changed(&error) => {
                        drop(repo.reload(Field::Log, cx));
                    }
                    Err(error) => repo.more.error = Some(error),
                }
                cx.emit(Changed(Part::More));
            });
        }));
        cx.emit(Changed(Part::More));
    }
}

/// Whether `error` says the log's order changed as more of it was read (see
/// `Error::LogOrderChanged`).
fn is_log_order_changed(error: &DataError) -> bool {
    error
        .api()
        .is_some_and(|api| matches!(api.error(), gitto_git::Error::LogOrderChanged))
}
