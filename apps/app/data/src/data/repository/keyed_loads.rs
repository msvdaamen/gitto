//! What a [`Repository`] loads by a key (see `super::super::keyed`): where a commit was pushed, a
//! file's patch, a conflict, a commit and its files and message.

use futures::FutureExt as _;
use futures::channel::oneshot;
use gitto_git::features::conflicts::schema::Conflict;
use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::features::history::schema::Commit;
use gitto_git::input::{FileInput, FullSha, NonEmpty, Sha};
use gitto_git::schema::FileStatus;
use gpui_kit::{Context, Task};

use super::{Part, Repository, failed, loaded_result};
use crate::data::diff::{ChangedFiles, FilesOf};
use crate::data::error::DataError;
use crate::data::file_diff::FilePatch;
use crate::data::keyed::Hold;
use crate::data::loaded::Loaded;
use crate::data::opaque::Opaque;
use gitto_ui_logic::git::diff_source::{DiffSource, diff_file_key};

impl Repository {
    /// Which remote branch has the commit `sha`, if that's loaded.
    pub fn pushed_to(&self, sha: &str) -> Option<&Loaded<Option<String>>> {
        self.pushed_to.get(&sha.to_owned())
    }

    /// Holds which remote branch has the commit `sha`, loading it unless it's loaded or loading,
    /// and loading it again as the refs change, while it's held.
    pub fn hold_pushed_to(&mut self, sha: &str, cx: &mut Context<Self>) -> Hold {
        let hold = self.pushed_to.hold(sha.to_owned());
        let loaded = self.pushed_to.entry(sha.to_owned());
        if loaded.wants_load() {
            drop(self.load_pushed_to(sha.to_owned(), cx));
        }
        hold
    }

    pub(super) fn load_pushed_to(
        &mut self,
        sha: String,
        cx: &mut Context<Self>,
    ) -> Option<oneshot::Receiver<()>> {
        let fetch = match Sha::parse(sha.clone()) {
            Ok(commit) => self.git.pushed_to(commit),
            Err(error) => failed(self.git.invalid(error)),
        };
        self.pushed_to.entry(sha.clone());
        self.start(
            Part::PushedTo,
            move |repo| repo.pushed_to.get_mut(&sha),
            fetch,
            cx,
        )
    }

    /// The patch of the file at `path` in `source`, if that's loaded.
    pub fn patch(&self, source: &DiffSource, path: &str) -> Option<&Loaded<FilePatch>> {
        self.patches.get(&(source.clone(), path.to_owned()))
    }

    /// Holds the patch of `file` in `source`, loading it unless it's loaded or loading, and
    /// loading it again as the uncommitted changes change, if it's of those, while it's held.
    pub fn hold_patch(
        &mut self,
        source: &DiffSource,
        file: &ChangedFile,
        cx: &mut Context<Self>,
    ) -> Hold {
        let key = (source.clone(), file.path.clone());
        let hold = self.patches.hold(key.clone());
        let loaded = self.patches.entry(key);
        if loaded.wants_load() {
            drop(self.load_patch(source.clone(), file.clone(), cx));
        }
        hold
    }

    /// The patch of `file` in `source`: what's loaded, or what loads; e.g. for a file about to be
    /// opened.
    pub fn fetch_patch(
        &mut self,
        source: &DiffSource,
        file: &ChangedFile,
        cx: &mut Context<Self>,
    ) -> Task<Result<FilePatch, DataError>> {
        let key = (source.clone(), file.path.clone());
        let wait = {
            let loaded = self.patches.entry(key.clone());
            if loaded.wants_load() {
                self.load_patch(source.clone(), file.clone(), cx)
            } else if loaded.is_fetching() {
                loaded.wait()
            } else {
                None
            }
        };
        cx.spawn(async move |this, cx| {
            if let Some(wait) = wait {
                let _ = wait.await;
            }
            this.read_with(cx, |repo, _| loaded_result(repo.patches.get(&key)))?
        })
    }

    pub(super) fn load_patch(
        &mut self,
        source: DiffSource,
        file: ChangedFile,
        cx: &mut Context<Self>,
    ) -> Option<oneshot::Receiver<()>> {
        let input = FileInput {
            path: file.path.clone(),
            orig_path: file.orig_path.clone(),
        };
        let patch = match &source {
            DiffSource::Commit { sha } => match Sha::parse(sha.clone()) {
                Ok(commit) => self.git.commit_file_patch(commit, input),
                Err(error) => failed(self.git.invalid(error)),
            },
            DiffSource::Stash { sha } => match FullSha::parse(sha.clone()) {
                Ok(stash) => self.git.stash_file_patch(stash, input),
                Err(error) => failed(self.git.invalid(error)),
            },
            DiffSource::Unstaged => {
                let untracked = file.status == FileStatus::Untracked;
                self.git.unstaged_file_patch(input, untracked)
            }
            DiffSource::Staged => self.git.staged_file_patch(input),
        };
        let key = (source.clone(), file.path.clone());
        let fetch = async move {
            let patch = patch.await?;
            Ok(FilePatch {
                key: diff_file_key(&source, &file.path),
                file,
                patch: patch.into(),
            })
        }
        .boxed();
        self.patches.entry(key.clone());
        self.start(
            Part::Patches,
            move |repo| repo.patches.get_mut(&key),
            fetch,
            cx,
        )
    }

    /// The conflicted file at `path`, if it's loaded.
    pub fn conflict(&self, path: &str) -> Option<&Loaded<Conflict>> {
        self.conflicts.get(&path.to_owned())
    }

    /// Holds the conflicted file at `path`, loading it unless it's loaded or loading, and loading
    /// it again as the uncommitted changes change, while it's held.
    pub fn hold_conflict(&mut self, path: &str, cx: &mut Context<Self>) -> Hold {
        let hold = self.conflicts.hold(path.to_owned());
        let loaded = self.conflicts.entry(path.to_owned());
        if loaded.wants_load() {
            drop(self.load_conflict(path.to_owned(), cx));
        }
        hold
    }

    /// Loads the conflicted file at `path` again: it changed on disk since it was read, say.
    pub fn reload_conflict(
        &mut self,
        path: &str,
        cx: &mut Context<Self>,
    ) -> Option<oneshot::Receiver<()>> {
        self.load_conflict(path.to_owned(), cx)
    }

    /// The conflicted file at `path`: what's loaded, or what loads; e.g. for one about to be
    /// opened.
    pub fn fetch_conflict(
        &mut self,
        path: &str,
        cx: &mut Context<Self>,
    ) -> Task<Result<Conflict, DataError>> {
        let path = path.to_owned();
        let wait = {
            let loaded = self.conflicts.entry(path.clone());
            if loaded.wants_load() {
                self.load_conflict(path.clone(), cx)
            } else if loaded.is_fetching() {
                loaded.wait()
            } else {
                None
            }
        };
        cx.spawn(async move |this, cx| {
            if let Some(wait) = wait {
                let _ = wait.await;
            }
            this.read_with(cx, |repo, _| loaded_result(repo.conflicts.get(&path)))?
        })
    }

    pub(super) fn load_conflict(
        &mut self,
        path: String,
        cx: &mut Context<Self>,
    ) -> Option<oneshot::Receiver<()>> {
        let fetch = match NonEmpty::parse(path.clone()) {
            Ok(file) => self.git.conflict(file),
            Err(error) => failed(self.git.invalid(error)),
        };
        self.conflicts.entry(path.clone());
        self.start(
            Part::Conflicts,
            move |repo| repo.conflicts.get_mut(&path),
            fetch,
            cx,
        )
    }

    /// The contents of the object `oid`, a file's at some version: what's loaded, or what loads;
    /// to show more of a file around its changes, say.
    pub fn fetch_blob(
        &mut self,
        oid: &str,
        cx: &mut Context<Self>,
    ) -> Task<Result<String, DataError>> {
        let oid = oid.to_owned();
        let wait = {
            let loaded = self.blobs.entry(oid.clone());
            if loaded.is_fetching() {
                loaded.wait()
            } else if loaded.is_stale() {
                let fetch = match FullSha::parse(oid.clone()) {
                    Ok(blob) => self.git.blob(blob),
                    Err(error) => failed(self.git.invalid(error)),
                };
                let key = oid.clone();
                self.start(
                    Part::Patches,
                    move |repo| repo.blobs.get_mut(&key),
                    fetch,
                    cx,
                )
            } else {
                None
            }
        };
        cx.spawn(async move |this, cx| {
            if let Some(wait) = wait {
                let _ = wait.await;
            }
            this.read_with(cx, |repo, _| loaded_result(repo.blobs.get(&oid)))?
        })
    }

    /// The commit `sha`, if it's loaded, or in the log: the log has all of a commit, so git isn't
    /// asked for it again.
    pub fn commit(&self, sha: &str) -> Option<&Commit> {
        self.commits
            .get(&sha.to_owned())
            .and_then(Loaded::data)
            .or_else(|| self.commit_in_log(sha))
    }

    /// Why loading the commit `sha` failed, if it did.
    pub fn commit_error(&self, sha: &str) -> Option<&DataError> {
        self.commits.get(&sha.to_owned()).and_then(Loaded::error)
    }

    fn commit_in_log(&self, sha: &str) -> Option<&Commit> {
        self.log
            .data()?
            .commits
            .iter()
            .find(|commit| commit.sha == sha)
    }

    /// Holds the commit `sha`, loading it unless it's loaded or in the log.
    pub fn hold_commit(&mut self, sha: &str, cx: &mut Context<Self>) -> Hold {
        let hold = self.commits.hold(sha.to_owned());
        if let Some(commit) = self.commit_in_log(sha).cloned() {
            self.commits.set(sha.to_owned(), commit);
        } else if self.commits.entry(sha.to_owned()).is_stale()
            && !self.commits.entry(sha.to_owned()).is_fetching()
        {
            let sha = sha.to_owned();
            let fetch = match Sha::parse(sha.clone()) {
                Ok(commit) => self.git.commit(commit),
                Err(error) => failed(self.git.invalid(error)),
            };
            drop(self.start(
                Part::Commits,
                move |repo| repo.commits.get_mut(&sha),
                fetch,
                cx,
            ));
        }
        hold
    }

    /// The files the commit or stash `sha` changed, if they're loaded.
    pub fn files(&self, of: FilesOf, sha: &str) -> Option<&Loaded<ChangedFiles>> {
        self.files.get(&(of, sha.to_owned()))
    }

    /// Holds the files the commit or stash `sha` changed, loading them unless they're loaded.
    pub fn hold_files(&mut self, of: FilesOf, sha: &str, cx: &mut Context<Self>) -> Hold {
        let key = (of, sha.to_owned());
        let hold = self.files.hold(key.clone());
        let loaded = self.files.entry(key.clone());
        if loaded.is_stale() && !loaded.is_fetching() {
            let sha = sha.to_owned();
            let files = match of {
                FilesOf::Commit => match Sha::parse(sha.clone()) {
                    Ok(commit) => self.git.commit_files(commit),
                    Err(error) => failed(self.git.invalid(error)),
                },
                FilesOf::Stash => match FullSha::parse(sha.clone()) {
                    Ok(stash) => self.git.stash_files(stash),
                    Err(error) => failed(self.git.invalid(error)),
                },
            };
            let fetch = async move {
                Ok(ChangedFiles {
                    sha,
                    files: Opaque::new(files.await?),
                })
            }
            .boxed();
            drop(self.start(
                Part::Commits,
                move |repo| repo.files.get_mut(&key),
                fetch,
                cx,
            ));
        }
        hold
    }

    /// The commit `sha`'s whole message, if it's loaded.
    pub fn message(&self, sha: &str) -> Option<&Loaded<String>> {
        self.messages.get(&sha.to_owned())
    }

    /// Holds the commit `sha`'s whole message, loading it unless it's loaded.
    pub fn hold_message(&mut self, sha: &str, cx: &mut Context<Self>) -> Hold {
        let hold = self.messages.hold(sha.to_owned());
        let loaded = self.messages.entry(sha.to_owned());
        if loaded.is_stale() && !loaded.is_fetching() {
            let sha = sha.to_owned();
            let fetch = match Sha::parse(sha.clone()) {
                Ok(commit) => self.git.commit_message(commit),
                Err(error) => failed(self.git.invalid(error)),
            };
            drop(self.start(
                Part::Commits,
                move |repo| repo.messages.get_mut(&sha),
                fetch,
                cx,
            ));
        }
        hold
    }
}
