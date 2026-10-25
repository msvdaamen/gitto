//! What a commit or a stash changed, and the uncommitted changes' lists. Ported from
//! `git/queries/diff.ts`.

use std::collections::HashSet;

use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::features::status::schema::Uncommitted;
use gitto_git::input::{FullSha, RepositoryId, Sha};
use gitto_git::schema::FileStatus;
use gpui_kit::{App, Context, Entity, Task};

use super::call::loads;
use super::client::{QueryError, QueryOptions, StaleTime};
use super::keyed::Hold;
use super::keys;
use super::loaded::Loaded;
use super::opaque::{Opaque, memoized};
use super::query::Query;
use super::repository::{Repositories, Repository};
use super::status::uncommitted_options;
use gitto_ui::git::changes::{LineCounts, line_totals};

/// Whose changed files to load: a commit's, compared to its first parent, or a stash's, compared
/// to the commit it was made on (TS's pair of a query key and a `FilesFetcher`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FilesOf {
    Commit,
    Stash,
}

/// The files a commit or stash changed, with the SHA they're of: while another one's load, the
/// previous ones stay on show, and say whose they are.
#[derive(Debug, Clone, PartialEq)]
pub struct ChangedFiles {
    pub sha: String,
    /// A commit can change tens of thousands of files.
    pub files: Opaque<Vec<ChangedFile>>,
}

/// Loads the files the commit or stash `sha` changed, which never change (`changedFilesQuery`).
/// While another one's load (see `Query::set_options`), the previous ones stay on show; they come
/// with the SHA they're of.
pub fn changed_files_options(
    id: &RepositoryId,
    sha: &str,
    of: FilesOf,
    enabled: bool,
    cx: &App,
) -> QueryOptions<ChangedFiles> {
    let sha = sha.to_owned();
    let key = match of {
        FilesOf::Commit => keys::commit_files(id, &sha),
        FilesOf::Stash => keys::stash_files(id, &sha),
    };
    QueryOptions::new(
        key,
        loads(cx, id, move |repo| {
            let sha = sha.clone();
            async move {
                let files = match of {
                    FilesOf::Commit => {
                        let commit = repo.input(Sha::parse(sha.clone()))?;
                        repo.commit_files(commit).await?
                    }
                    FilesOf::Stash => {
                        let stash = repo.input(FullSha::parse(sha.clone()))?;
                        repo.stash_files(stash).await?
                    }
                };
                Ok(ChangedFiles {
                    sha,
                    files: Opaque::new(files),
                })
            }
        }),
    )
    .stale_time(StaleTime::Infinite)
    .keep_previous_data()
    .enabled(enabled)
}

/// What a view shows of [`ChangedFiles`]: the files, their line totals, and whose they are.
#[derive(Debug, Clone, PartialEq)]
pub struct ShownFiles {
    /// The SHA whose files are on show: the previous one's, while another's load.
    pub shown_sha: String,
    pub files: Opaque<Vec<ChangedFile>>,
    pub totals: LineCounts,
}

/// [`ShownFiles`] of [`ChangedFiles`], counted once per list.
pub fn select_shown_files() -> impl Fn(&ChangedFiles) -> ShownFiles + 'static {
    let totals = memoized(|files: &Opaque<Vec<ChangedFile>>| line_totals(files));
    move |changed| ShownFiles {
        shown_sha: changed.sha.clone(),
        files: changed.files.clone(),
        totals: totals(&changed.files),
    }
}

/// The files of a commit or stash that a view shows, from its repository: held while they're on
/// show, and, while another's load, the ones before kept on show, saying whose they are.
pub struct FilesOnShow {
    of: FilesOf,
    sha: String,
    _hold: Hold,
    select: Box<dyn Fn(&ChangedFiles) -> ShownFiles>,
    last: Option<ShownFiles>,
}

impl FilesOnShow {
    /// The files `of` `sha` changed, in `repository`.
    pub fn new(of: FilesOf, sha: &str, repository: &Entity<Repository>, cx: &mut App) -> Self {
        FilesOnShow {
            of,
            sha: sha.to_owned(),
            _hold: repository.update(cx, |repository, cx| repository.hold_files(of, sha, cx)),
            select: Box::new(select_shown_files()),
            last: None,
        }
    }

    /// Shows the files `sha` changed instead; the ones on show stay until they're loaded.
    pub fn set_sha(&mut self, sha: &str, repository: &Entity<Repository>, cx: &mut App) {
        if self.sha != sha {
            self.sha = sha.to_owned();
            let of = self.of;
            self._hold = repository.update(cx, |repository, cx| repository.hold_files(of, sha, cx));
        }
    }

    /// The files on show, as `repository` has them now: call as the view renders.
    pub fn data(&mut self, repository: &Repository) -> Option<ShownFiles> {
        if let Some(files) = repository.files(self.of, &self.sha).and_then(Loaded::data) {
            self.last = Some((self.select)(files));
        }
        self.last.clone()
    }

    /// Why the files `sha` changed couldn't be loaded, if they couldn't.
    pub fn error(&self, repository: &Repository) -> Option<QueryError> {
        repository
            .files(self.of, &self.sha)
            .and_then(Loaded::error)
            .cloned()
    }
}

/// The files the commit or stash `sha` changed (see [`changed_files_options`];
/// `useChangedFiles`); [`ShownFiles::shown_sha`] says whose are on show, so what's shown with them
/// can match. To show another's, `Query::set_options` with [`changed_files_options`].
pub fn changed_files_query<V: 'static>(
    id: &RepositoryId,
    sha: &str,
    of: FilesOf,
    cx: &mut Context<V>,
) -> Query<ChangedFiles, ShownFiles> {
    Query::with_select(
        changed_files_options(id, sha, of, true, cx),
        select_shown_files(),
        cx,
    )
}

/// Files changed by a commit, compared to its first parent (`useCommitFiles`).
pub fn commit_files_query<V: 'static>(
    id: &RepositoryId,
    sha: &str,
    cx: &mut Context<V>,
) -> Query<ChangedFiles, ShownFiles> {
    changed_files_query(id, sha, FilesOf::Commit, cx)
}

/// A file's contents by their object name, e.g. to show more of it around a patch's changes
/// (`fetchBlob`).
pub fn fetch_blob(id: &RepositoryId, oid: &str, cx: &mut App) -> Task<Result<String, QueryError>> {
    Repositories::open(id, cx).update(cx, |repository, cx| repository.fetch_blob(oid, cx))
}

/// The lists in the uncommitted changes: what's staged for the next commit and what isn't, and
/// whether their lines were counted (`useWorkingTreeLists`). Made once per status loaded (see
/// [`select_working_tree_lists`]), and equal only to those of the same one.
#[derive(Debug, Clone)]
pub struct WorkingTreeLists {
    status: Opaque<Uncommitted>,
    staged: Opaque<Vec<ChangedFile>>,
    unstaged: Opaque<Vec<ChangedFile>>,
    marker_free: Opaque<HashSet<String>>,
}

impl PartialEq for WorkingTreeLists {
    fn eq(&self, other: &Self) -> bool {
        Opaque::ptr_eq(&self.status, &other.status)
    }
}

impl WorkingTreeLists {
    /// The lists of `status`.
    pub fn of(status: &Opaque<Uncommitted>) -> Self {
        WorkingTreeLists {
            status: status.clone(),
            staged: Opaque::new(staged_files(&status.changes.staged)),
            unstaged: Opaque::new(status.changes.unstaged.clone()),
            marker_free: Opaque::new(status.changes.marker_free.iter().cloned().collect()),
        }
    }

    /// What's staged for the next commit (see [`staged_files`]).
    pub fn staged(&self) -> &Opaque<Vec<ChangedFile>> {
        &self.staged
    }

    pub fn unstaged(&self) -> &Opaque<Vec<ChangedFile>> {
        &self.unstaged
    }

    /// Whether there are too many files for their lines to have been counted.
    pub fn uncounted(&self) -> bool {
        self.status.changes.uncounted
    }

    /// The conflicted files with no conflict markers left, ready to be marked resolved.
    pub fn marker_free(&self) -> &HashSet<String> {
        &self.marker_free
    }

    /// The whole status they're of.
    pub fn status(&self) -> &Opaque<Uncommitted> {
        &self.status
    }
}

/// [`WorkingTreeLists::of`], made once per status: for `Query::with_select`.
pub fn select_working_tree_lists() -> impl Fn(&Opaque<Uncommitted>) -> WorkingTreeLists + 'static {
    memoized(WorkingTreeLists::of)
}

/// The uncommitted changes' lists (see [`WorkingTreeLists`]); only loaded while `enabled`. Whether
/// they're loaded is whether `Query::data` has them.
pub fn working_tree_lists_query<V: 'static>(
    id: &RepositoryId,
    enabled: bool,
    cx: &mut Context<V>,
) -> Query<Opaque<Uncommitted>, WorkingTreeLists> {
    let options = uncommitted_options(id, super::status::git_status(cx)).enabled(enabled);
    Query::with_select(options, select_working_tree_lists(), cx)
}

/// The uncommitted changes, split into what's staged for the next commit and what isn't
/// (`useWorkingTreeChanges`). They come with the status, so this shares its query.
pub fn working_tree_changes_query<V: 'static>(
    id: &RepositoryId,
    cx: &mut Context<V>,
) -> Query<Opaque<Uncommitted>, WorkingTreeLists> {
    working_tree_lists_query(id, true, cx)
}

/// The staged files to list: a conflict shows up on both sides, but it's resolved by staging it.
pub fn staged_files(staged: &[ChangedFile]) -> Vec<ChangedFile> {
    staged
        .iter()
        .filter(|file| file.status != FileStatus::Conflicted)
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, status: FileStatus) -> ChangedFile {
        ChangedFile {
            path: path.into(),
            status,
            orig_path: None,
            additions: Some(1),
            deletions: Some(2),
        }
    }

    #[test]
    fn lists_a_conflict_among_the_unstaged_files_only() {
        let staged = [
            file("a.txt", FileStatus::Modified),
            file("b.txt", FileStatus::Conflicted),
        ];
        assert_eq!(staged_files(&staged), [file("a.txt", FileStatus::Modified)]);
    }

    #[test]
    fn counts_the_shown_files_lines_once_per_list() {
        let select = select_shown_files();
        let changed = ChangedFiles {
            sha: "abc".into(),
            files: Opaque::new(vec![
                file("a.txt", FileStatus::Modified),
                file("b.txt", FileStatus::Added),
            ]),
        };
        let shown = select(&changed);
        assert_eq!(shown.shown_sha, "abc");
        assert_eq!(
            shown.totals,
            LineCounts {
                additions: 2,
                deletions: 4
            }
        );
        assert_eq!(select(&changed), shown);
    }
}
