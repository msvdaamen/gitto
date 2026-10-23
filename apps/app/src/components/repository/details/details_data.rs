//! What the details load and change, in one place: git's, through the data layer, in the app; a
//! test's own, made up, in its tests (TS's tests mocked `rpc` instead). GPUI's test scheduler
//! refuses wakeups from other threads, so a GPUI test can't run real git (see `crate::data`).

use std::rc::Rc;
use std::sync::Arc;

use gitto_git::features::history::schema::Commit;
use gitto_git::features::operation::schema::Operation;
use gitto_git::features::stash::schema::Stash;
use gitto_git::input::RepositoryId;
use gpui_kit::{App, Entity};

use crate::data::Mutation;
use crate::data::QueryOptions;
use crate::data::commit::{
    NewCommit, commit_message_options, create_commit_mutation, pushed_to_options,
};
use crate::data::diff::{ChangedFiles, FilesOf, changed_files_options};
use crate::data::history::commit_details_options;
use crate::data::progress::operation_in_progress_options;
use crate::data::staging::{
    DiscardTarget, StagingTarget, discard_mutation, stage_mutation, unstage_mutation,
};
use crate::data::stash::stashes_options;
use crate::data::status::{GetStatus, git_status};

/// A query's options, for a repository and a commit or stash's SHA.
type ShaOptions<T> = Rc<dyn Fn(&RepositoryId, &str, &App) -> QueryOptions<T>>;
/// A query's options, for a repository.
type RepositoryOptions<T> = Rc<dyn Fn(&RepositoryId, &App) -> QueryOptions<T>>;
/// Makes a mutation, for a view to hold and observe.
type MakeMutation<I> = Rc<dyn Fn(&mut App) -> Entity<Mutation<I>>>;

/// A commit's or a stash's changed files' options (see `changed_files_options`).
pub type ChangedFilesOptions =
    Rc<dyn Fn(&RepositoryId, &str, FilesOf, bool, &App) -> QueryOptions<ChangedFiles>>;

/// What the details load (a commit, the files it changed, the stashes, the status, the operation
/// under way, the last commit's message and where it was pushed) and change (committing, staging,
/// unstaging, discarding). Cheap to clone: the details hand it to each of their parts.
#[derive(Clone)]
pub struct DetailsData {
    commit: ShaOptions<Commit>,
    changed_files: ChangedFilesOptions,
    stashes: RepositoryOptions<Arc<[Stash]>>,
    status: Rc<dyn Fn(&App) -> GetStatus>,
    operation: RepositoryOptions<Option<Operation>>,
    commit_message: ShaOptions<String>,
    pushed_to: ShaOptions<Option<String>>,
    create_commit: MakeMutation<(RepositoryId, NewCommit)>,
    stage: MakeMutation<(RepositoryId, StagingTarget)>,
    unstage: MakeMutation<(RepositoryId, StagingTarget)>,
    discard: MakeMutation<(RepositoryId, DiscardTarget)>,
}

impl DetailsData {
    /// Git's, through the data layer: what the app shows.
    pub fn git() -> Self {
        DetailsData {
            commit: Rc::new(commit_details_options),
            changed_files: Rc::new(|id, sha, of, enabled, cx| {
                changed_files_options(id, sha, of, enabled, cx)
            }),
            stashes: Rc::new(stashes_options),
            status: Rc::new(git_status),
            operation: Rc::new(operation_in_progress_options),
            commit_message: Rc::new(commit_message_options),
            pushed_to: Rc::new(pushed_to_options),
            create_commit: Rc::new(create_commit_mutation),
            stage: Rc::new(stage_mutation),
            unstage: Rc::new(unstage_mutation),
            discard: Rc::new(discard_mutation),
        }
    }

    pub fn commit(&self, id: &RepositoryId, sha: &str, cx: &App) -> QueryOptions<Commit> {
        (self.commit)(id, sha, cx)
    }

    pub fn changed_files(
        &self,
        id: &RepositoryId,
        sha: &str,
        of: FilesOf,
        cx: &App,
    ) -> QueryOptions<ChangedFiles> {
        (self.changed_files)(id, sha, of, true, cx)
    }

    pub fn stashes(&self, id: &RepositoryId, cx: &App) -> QueryOptions<Arc<[Stash]>> {
        (self.stashes)(id, cx)
    }

    /// How the status is read (see `GetStatus`), for `uncommitted_options`.
    pub fn status(&self, cx: &App) -> GetStatus {
        (self.status)(cx)
    }

    pub fn operation(&self, id: &RepositoryId, cx: &App) -> QueryOptions<Option<Operation>> {
        (self.operation)(id, cx)
    }

    pub fn commit_message(&self, id: &RepositoryId, sha: &str, cx: &App) -> QueryOptions<String> {
        (self.commit_message)(id, sha, cx)
    }

    pub fn pushed_to(
        &self,
        id: &RepositoryId,
        sha: &str,
        cx: &App,
    ) -> QueryOptions<Option<String>> {
        (self.pushed_to)(id, sha, cx)
    }

    pub fn create_commit(&self, cx: &mut App) -> Entity<Mutation<(RepositoryId, NewCommit)>> {
        (self.create_commit)(cx)
    }

    pub fn stage(&self, cx: &mut App) -> Entity<Mutation<(RepositoryId, StagingTarget)>> {
        (self.stage)(cx)
    }

    pub fn unstage(&self, cx: &mut App) -> Entity<Mutation<(RepositoryId, StagingTarget)>> {
        (self.unstage)(cx)
    }

    pub fn discard(&self, cx: &mut App) -> Entity<Mutation<(RepositoryId, DiscardTarget)>> {
        (self.discard)(cx)
    }
}

#[cfg(test)]
impl DetailsData {
    pub fn with_commit(
        mut self,
        commit: impl Fn(&RepositoryId, &str, &App) -> QueryOptions<Commit> + 'static,
    ) -> Self {
        self.commit = Rc::new(commit);
        self
    }

    pub fn with_changed_files(
        mut self,
        files: impl Fn(&RepositoryId, &str, FilesOf, bool, &App) -> QueryOptions<ChangedFiles> + 'static,
    ) -> Self {
        self.changed_files = Rc::new(files);
        self
    }

    pub fn with_stashes(
        mut self,
        stashes: impl Fn(&RepositoryId, &App) -> QueryOptions<Arc<[Stash]>> + 'static,
    ) -> Self {
        self.stashes = Rc::new(stashes);
        self
    }

    pub fn with_status(mut self, status: GetStatus) -> Self {
        self.status = Rc::new(move |_| Rc::clone(&status));
        self
    }

    pub fn with_operation(
        mut self,
        operation: impl Fn(&RepositoryId, &App) -> QueryOptions<Option<Operation>> + 'static,
    ) -> Self {
        self.operation = Rc::new(operation);
        self
    }

    pub fn with_commit_message(
        mut self,
        message: impl Fn(&RepositoryId, &str, &App) -> QueryOptions<String> + 'static,
    ) -> Self {
        self.commit_message = Rc::new(message);
        self
    }

    pub fn with_pushed_to(
        mut self,
        pushed_to: impl Fn(&RepositoryId, &str, &App) -> QueryOptions<Option<String>> + 'static,
    ) -> Self {
        self.pushed_to = Rc::new(pushed_to);
        self
    }

    pub fn with_create_commit(
        mut self,
        create: impl Fn(&mut App) -> Entity<Mutation<(RepositoryId, NewCommit)>> + 'static,
    ) -> Self {
        self.create_commit = Rc::new(create);
        self
    }

    pub fn with_staging(
        mut self,
        stage: impl Fn(&mut App) -> Entity<Mutation<(RepositoryId, StagingTarget)>> + 'static,
        unstage: impl Fn(&mut App) -> Entity<Mutation<(RepositoryId, StagingTarget)>> + 'static,
        discard: impl Fn(&mut App) -> Entity<Mutation<(RepositoryId, DiscardTarget)>> + 'static,
    ) -> Self {
        self.stage = Rc::new(stage);
        self.unstage = Rc::new(unstage);
        self.discard = Rc::new(discard);
        self
    }
}
