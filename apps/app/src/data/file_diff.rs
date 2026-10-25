//! A file's changes in a commit, a stash, or the uncommitted changes, and the files to step
//! through. Ported from `git/queries/file-diff.ts`.

use std::sync::Arc;

use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::features::diff::working_tree::{SaveExpectation, WorkingTreeFile};
use gitto_git::features::status::schema::Uncommitted;
use gitto_git::input::{FileInput, FullSha, RepositoryId, Sha};
use gitto_git::schema::FileStatus;
use gpui_kit::{App, AppContext as _, Context, Task};

use super::call::{call, loads};
use super::client::{QueryClient, QueryError, QueryOptions, StaleTime};
use super::diff::{ChangedFiles, FilesOf, ShownFiles, WorkingTreeLists, changed_files_options};
use super::diff::{select_shown_files, working_tree_lists_query};
use super::keys::{self, UncommittedSide};
use super::opaque::Opaque;
use super::query::Query;
use gitto_ui::git::diff_source::{DiffSource, diff_file_key, is_uncommitted};

/// The files whose changes a [`DiffSource`] has, to step through, kept up to date for uncommitted
/// ones (`useDiffSourceFiles`). A view keeps one, made in its constructor, and tells it when the
/// source changes ([`set_source`](Self::set_source)). Only the source's own query loads; the
/// others are disabled.
pub struct DiffSourceFiles {
    source: DiffSource,
    commit: Query<ChangedFiles, ShownFiles>,
    stash: Query<ChangedFiles, ShownFiles>,
    working_tree: Query<Opaque<Uncommitted>, WorkingTreeLists>,
}

impl DiffSourceFiles {
    pub fn new<V: 'static>(id: &RepositoryId, source: DiffSource, cx: &mut Context<V>) -> Self {
        let commit = Query::with_select(
            source_files_options(id, &source, FilesOf::Commit, cx),
            select_shown_files(),
            cx,
        );
        let stash = Query::with_select(
            source_files_options(id, &source, FilesOf::Stash, cx),
            select_shown_files(),
            cx,
        );
        let working_tree = working_tree_lists_query(id, is_uncommitted(&source), cx);
        DiffSourceFiles {
            source,
            commit,
            stash,
            working_tree,
        }
    }

    /// Shows the files of `source` in the repository `id` instead. While another commit's or
    /// stash's load, the last one's stay, but [`loaded`](Self::loaded) says they aren't its.
    pub fn set_source<V: 'static>(
        &mut self,
        id: &RepositoryId,
        source: DiffSource,
        cx: &mut Context<V>,
    ) {
        self.commit
            .set_options(source_files_options(id, &source, FilesOf::Commit, cx), cx);
        self.stash
            .set_options(source_files_options(id, &source, FilesOf::Stash, cx), cx);
        self.working_tree.set_enabled(is_uncommitted(&source), cx);
        self.source = source;
        cx.notify();
    }

    pub fn source(&self) -> &DiffSource {
        &self.source
    }

    /// The files: none until they've loaded.
    pub fn files(&self) -> Opaque<Vec<ChangedFile>> {
        let files = match &self.source {
            DiffSource::Commit { .. } => self.commit.data().map(|shown| shown.files),
            DiffSource::Stash { .. } => self.stash.data().map(|shown| shown.files),
            DiffSource::Unstaged => self
                .working_tree
                .data()
                .map(|lists| lists.unstaged().clone()),
            DiffSource::Staged => self.working_tree.data().map(|lists| lists.staged().clone()),
        };
        files.unwrap_or_else(|| Opaque::new(Vec::new()))
    }

    /// Whether [`files`](Self::files) are the source's, rather than none yet or another commit's
    /// still on show.
    pub fn loaded(&self) -> bool {
        match &self.source {
            DiffSource::Commit { sha } => self
                .commit
                .data()
                .is_some_and(|shown| shown.shown_sha == *sha),
            DiffSource::Stash { sha } => self
                .stash
                .data()
                .is_some_and(|shown| shown.shown_sha == *sha),
            DiffSource::Unstaged | DiffSource::Staged => self.working_tree.data().is_some(),
        }
    }

    /// The files' lines weren't counted, so one without line counts isn't known to be binary.
    pub fn uncounted(&self) -> bool {
        is_uncommitted(&self.source)
            && self
                .working_tree
                .data()
                .is_some_and(|lists| lists.uncounted())
    }

    /// The staged files, when the source is uncommitted; none until they've loaded.
    pub fn staged(&self) -> Opaque<Vec<ChangedFile>> {
        self.working_tree
            .data()
            .map(|lists| lists.staged().clone())
            .unwrap_or_else(|| Opaque::new(Vec::new()))
    }

    /// The unstaged files, when the source is uncommitted; none until they've loaded.
    pub fn unstaged(&self) -> Opaque<Vec<ChangedFile>> {
        self.working_tree
            .data()
            .map(|lists| lists.unstaged().clone())
            .unwrap_or_else(|| Opaque::new(Vec::new()))
    }
}

/// The options of the files of `of` that [`DiffSourceFiles`] keeps for `source`: the commit's or
/// stash's own if it's one of those, disabled otherwise.
fn source_files_options(
    id: &RepositoryId,
    source: &DiffSource,
    of: FilesOf,
    cx: &App,
) -> QueryOptions<ChangedFiles> {
    let (sha, enabled) = match source {
        DiffSource::Commit { sha } => (sha.as_str(), of == FilesOf::Commit),
        DiffSource::Stash { sha } => (sha.as_str(), of == FilesOf::Stash),
        DiffSource::Unstaged | DiffSource::Staged => ("", false),
    };
    changed_files_options(id, sha, of, enabled, cx)
}

/// One file's patch in a source, with the file and its `DiffSource::file_key`, so what's shown can
/// match.
#[derive(Debug, Clone, PartialEq)]
pub struct FilePatch {
    pub key: String,
    pub file: ChangedFile,
    /// Shared: a patch can be megabytes, and it's read every time it's rendered.
    pub patch: Arc<str>,
}

/// Where the patch of the file at `path` in `source` is cached.
pub fn file_patch_key(id: &RepositoryId, source: &DiffSource, path: &str) -> super::QueryKey {
    match source {
        DiffSource::Unstaged => keys::uncommitted_file_patch(id, UncommittedSide::Unstaged, path),
        DiffSource::Staged => keys::uncommitted_file_patch(id, UncommittedSide::Staged, path),
        DiffSource::Commit { sha } => keys::commit_file_patch(id, sha, path),
        DiffSource::Stash { sha } => keys::stash_file_patch(id, sha, path),
    }
}

/// Loads the patch of `file`'s changes in `source` (`filePatchQuery`).
pub fn file_patch_options(
    id: &RepositoryId,
    source: &DiffSource,
    file: &ChangedFile,
    cx: &App,
) -> QueryOptions<FilePatch> {
    let key = file_patch_key(id, source, &file.path);
    let source = source.clone();
    let file = file.clone();
    QueryOptions::new(
        key,
        loads(cx, id, move |repo| {
            let source = source.clone();
            let file = file.clone();
            async move {
                let input = FileInput {
                    path: file.path.clone(),
                    orig_path: file.orig_path.clone(),
                };
                let patch = match &source {
                    DiffSource::Commit { sha } => {
                        let commit = repo.input(Sha::parse(sha.clone()))?;
                        repo.commit_file_patch(commit, input).await?
                    }
                    DiffSource::Stash { sha } => {
                        let stash = repo.input(FullSha::parse(sha.clone()))?;
                        repo.stash_file_patch(stash, input).await?
                    }
                    DiffSource::Unstaged => {
                        let untracked = file.status == FileStatus::Untracked;
                        repo.unstaged_file_patch(input, untracked).await?
                    }
                    DiffSource::Staged => repo.staged_file_patch(input).await?,
                };
                Ok(FilePatch {
                    key: diff_file_key(&source, &file.path),
                    file,
                    patch: patch.into(),
                })
            }
        }),
    )
    // Only reloaded once it's invalidated: never for a commit's or a stash's, which don't change,
    // and for an uncommitted one (under `keys::uncommitted`) like the status, whenever the working
    // tree or the index changes.
    .stale_time(StaleTime::Infinite)
}

/// [`file_patch_options`] as [`file_patch_query`] shows them, for `Query::set_options` when the
/// file on show changes.
pub fn file_patch_query_options(
    id: &RepositoryId,
    source: &DiffSource,
    file: &ChangedFile,
    enabled: bool,
    cx: &App,
) -> QueryOptions<FilePatch> {
    file_patch_options(id, source, file, cx)
        .keep_previous_data()
        .enabled(enabled)
}

/// The patch of `file`'s changes in `source` (`useFilePatch`). While another file's loads (see
/// `Query::set_options`), the previous one's stays on show: it comes with the file it's of.
pub fn file_patch_query<V: 'static>(
    id: &RepositoryId,
    source: &DiffSource,
    file: &ChangedFile,
    enabled: bool,
    cx: &mut Context<V>,
) -> Query<FilePatch> {
    Query::new(file_patch_query_options(id, source, file, enabled, cx), cx)
}

/// Loads the patch [`file_patch_query`] would, ahead of it, e.g. for a file about to be opened
/// (`fetchFilePatch`).
pub fn fetch_file_patch(
    id: &RepositoryId,
    source: &DiffSource,
    file: &ChangedFile,
    cx: &mut App,
) -> Task<Result<FilePatch, QueryError>> {
    let options = file_patch_options(id, source, file, cx);
    QueryClient::fetch_query(options, cx)
}

/// A file's contents in the working tree, with their version, to show more of its unstaged
/// changes or edit them (`fetchWorkingTreeFile`). Not cached: they can change at any time, and
/// are read only to fill in a patch that's just been loaded.
pub fn fetch_working_tree_file(
    id: &RepositoryId,
    path: &str,
    cx: &mut App,
) -> Task<Result<WorkingTreeFile, QueryError>> {
    let path = path.to_owned();
    let reading = call(cx, id, move |repo| repo.working_tree_file(path));
    cx.background_spawn(async move { reading.await.map_err(Into::into) })
}

/// Saves a file's edits over it in the working tree, if it's still at `version` or to
/// `overwrite` it; resolves to the saved file's version (`saveWorkingTreeFile`). The watcher
/// reloads what changed with it.
pub fn save_working_tree_file(
    id: &RepositoryId,
    path: &str,
    contents: String,
    version: String,
    overwrite: bool,
    cx: &mut App,
) -> Task<Result<String, QueryError>> {
    let path = path.to_owned();
    let saving = call(cx, id, move |repo| {
        let expected = SaveExpectation { version, overwrite };
        repo.save_working_tree_file(path, contents, expected)
    });
    cx.background_spawn(async move { saving.await.map_err(Into::into) })
}
