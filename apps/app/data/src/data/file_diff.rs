//! A file's changes in a commit, a stash, or the uncommitted changes, and the files to step
//! through. Ported from `git/queries/file-diff.ts`.

use std::sync::Arc;

use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::features::diff::working_tree::{SaveExpectation, WorkingTreeFile};
use gitto_git::features::status::schema::Uncommitted;
use gitto_git::input::RepositoryId;
use gpui_kit::{App, AppContext as _, Entity, Task};

use super::diff::{ChangedFiles, FilesOf, ShownFiles, WorkingTreeLists};
use super::diff::{select_shown_files, select_working_tree_lists};
use super::error::DataError;
use super::keyed::Hold;
use super::loaded::Loaded;
use super::opaque::Opaque;
use super::repository::{Field, Repositories, Repository};
use gitto_ui_logic::git::diff_source::{DiffSource, is_uncommitted};

/// One file's patch in a source, with the file and its `DiffSource::file_key`, so what's shown can
/// match.
#[derive(Debug, Clone, PartialEq)]
pub struct FilePatch {
    pub key: String,
    pub file: ChangedFile,
    /// Shared: a patch can be megabytes, and it's read every time it's rendered.
    pub patch: Arc<str>,
}

/// Picks what's shown of a commit's or a stash's files, counting their lines once per list.
type SelectFiles = Box<dyn Fn(&ChangedFiles) -> ShownFiles>;
/// Splits the status into the uncommitted changes' lists, once per status.
type SelectLists = Box<dyn Fn(&Opaque<Uncommitted>) -> WorkingTreeLists>;

/// The files whose changes a [`DiffSource`] has, to step through, kept up to date for uncommitted
/// ones (`useDiffSourceFiles`). A view keeps one, made in its constructor, tells it when the
/// source changes ([`set_source`](Self::set_source)), and [`sync`](Self::sync)s it with what's
/// loaded before it reads it. A commit's or a stash's files are held while they're its source's.
pub struct DiffSourceFiles {
    repository: Entity<Repository>,
    source: DiffSource,
    _hold: Option<Hold>,
    select_files: SelectFiles,
    select_lists: SelectLists,
    /// A commit's or a stash's files, as last synced: while another one's load, the last one's.
    files: Option<ShownFiles>,
    /// The uncommitted changes' lists, as last synced.
    lists: Option<WorkingTreeLists>,
}

impl DiffSourceFiles {
    pub fn new(repository: &Entity<Repository>, source: DiffSource, cx: &mut App) -> Self {
        let mut files = DiffSourceFiles {
            repository: repository.clone(),
            source: source.clone(),
            _hold: None,
            select_files: Box::new(select_shown_files()),
            select_lists: Box::new(select_working_tree_lists()),
            files: None,
            lists: None,
        };
        files.set_source(source, cx);
        files
    }

    /// Shows the files of `source` instead. While another commit's or stash's load, the last
    /// one's stay, but [`loaded`](Self::loaded) says they aren't its.
    pub fn set_source(&mut self, source: DiffSource, cx: &mut App) {
        self._hold = self.repository.update(cx, |repository, cx| match &source {
            DiffSource::Commit { sha } => Some(repository.hold_files(FilesOf::Commit, sha, cx)),
            DiffSource::Stash { sha } => Some(repository.hold_files(FilesOf::Stash, sha, cx)),
            DiffSource::Unstaged | DiffSource::Staged => {
                repository.show(Field::Status, cx);
                None
            }
        });
        if let (DiffSource::Commit { .. }, DiffSource::Stash { .. })
        | (DiffSource::Stash { .. }, DiffSource::Commit { .. }) = (&self.source, &source)
        {
            self.files = None;
        }
        self.source = source;
        self.sync(cx);
    }

    /// Brings what it says up to date with what the repository has loaded.
    pub fn sync(&mut self, cx: &App) {
        let repository = self.repository.read(cx);
        let files = match &self.source {
            DiffSource::Commit { sha } => repository.files(FilesOf::Commit, sha),
            DiffSource::Stash { sha } => repository.files(FilesOf::Stash, sha),
            DiffSource::Unstaged | DiffSource::Staged => None,
        };
        if let Some(files) = files.and_then(Loaded::data) {
            self.files = Some((self.select_files)(files));
        }
        if let Some(status) = repository.status().data() {
            self.lists = Some((self.select_lists)(status));
        }
    }

    /// The files: none until they've loaded.
    pub fn files(&self) -> Opaque<Vec<ChangedFile>> {
        let files = match &self.source {
            DiffSource::Commit { .. } | DiffSource::Stash { .. } => {
                self.files.as_ref().map(|shown| shown.files.clone())
            }
            DiffSource::Unstaged => self.lists.as_ref().map(|lists| lists.unstaged().clone()),
            DiffSource::Staged => self.lists.as_ref().map(|lists| lists.staged().clone()),
        };
        files.unwrap_or_else(|| Opaque::new(Vec::new()))
    }

    /// Whether [`files`](Self::files) are the source's, rather than none yet or another commit's
    /// still on show.
    pub fn loaded(&self) -> bool {
        match &self.source {
            DiffSource::Commit { sha } | DiffSource::Stash { sha } => self
                .files
                .as_ref()
                .is_some_and(|shown| shown.shown_sha == *sha),
            DiffSource::Unstaged | DiffSource::Staged => self.lists.is_some(),
        }
    }

    /// The files' lines weren't counted, so one without line counts isn't known to be binary.
    pub fn uncounted(&self) -> bool {
        is_uncommitted(&self.source) && self.lists.as_ref().is_some_and(|lists| lists.uncounted())
    }

    /// The staged files, when the source is uncommitted; none until they've loaded.
    pub fn staged(&self) -> Opaque<Vec<ChangedFile>> {
        self.lists
            .as_ref()
            .map(|lists| lists.staged().clone())
            .unwrap_or_else(|| Opaque::new(Vec::new()))
    }

    /// The unstaged files, when the source is uncommitted; none until they've loaded.
    pub fn unstaged(&self) -> Opaque<Vec<ChangedFile>> {
        self.lists
            .as_ref()
            .map(|lists| lists.unstaged().clone())
            .unwrap_or_else(|| Opaque::new(Vec::new()))
    }
}

/// Loads the patch of `file`'s changes in `source` ahead of it being shown, e.g. for a file about
/// to be opened (`fetchFilePatch`).
pub fn fetch_file_patch(
    id: &RepositoryId,
    source: &DiffSource,
    file: &ChangedFile,
    cx: &mut App,
) -> Task<Result<FilePatch, DataError>> {
    Repositories::open(id, cx).update(cx, |repository, cx| {
        repository.fetch_patch(source, file, cx)
    })
}

/// A file's contents in the working tree, with their version, to show more of its unstaged
/// changes or edit them (`fetchWorkingTreeFile`). Not kept: they can change at any time, and
/// are read only to fill in a patch that's just been loaded.
pub fn fetch_working_tree_file(
    id: &RepositoryId,
    path: &str,
    cx: &mut App,
) -> Task<Result<WorkingTreeFile, DataError>> {
    let git = Repositories::open(id, cx).read(cx).git().clone();
    cx.background_spawn(git.working_tree_file(path.to_owned()))
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
) -> Task<Result<String, DataError>> {
    let git = Repositories::open(id, cx).read(cx).git().clone();
    let expected = SaveExpectation { version, overwrite };
    cx.background_spawn(git.save_working_tree_file(path.to_owned(), contents, expected))
}
