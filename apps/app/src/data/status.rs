//! The status and the uncommitted changes. Ported from `git/queries/status.ts`.
//!
//! The uncommitted changes come in one query (so the working tree is only walked once), but most
//! of the UI only shows the summary. Each `Query` keeps its own selection, so the file lists,
//! which can be huge, are only selected (and kept up to date) where they're shown, and shared
//! rather than copied (see `super::opaque`).

use std::rc::Rc;

use futures::FutureExt as _;
use futures::future::BoxFuture;
use gitto_git::ApiError;
use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::features::status::commands::get_status_since;
use gitto_git::features::status::schema::{Head, StatusReply, StatusSummary, Uncommitted};
use gitto_git::input::RepositoryId;
use gitto_git::schema::FileStatus;
use gpui_kit::{App, Context};

use super::client::{QueryClient, QueryOptions};
use super::keys;
use super::opaque::Opaque;
use super::query::Query;
use super::watch::unwatched;
use crate::services::Services;

/// Asks for the status of a repository, given the version of the one the caller has, if any
/// (`rpc.git.status.get`): the app's goes to git, tests' are made up.
pub type GetStatus =
    Rc<dyn Fn(RepositoryId, Option<String>) -> BoxFuture<'static, Result<StatusReply, ApiError>>>;

/// Git's `status.get` (see [`GetStatus`]).
pub fn git_status(cx: &App) -> GetStatus {
    let git = Services::global(cx).git().clone();
    Rc::new(move |id, since| {
        git.call("status.get", &id, move |repo| async move {
            get_status_since(&repo, since.as_deref()).await
        })
        .boxed()
    })
}

/// The uncommitted changes of the repository `id`, as `get` reads them: only sent again when
/// they changed, as the version of the status there is is passed along. When they didn't, the
/// query keeps the same data, so nothing selected from it changes or renders again.
pub fn uncommitted_options(id: &RepositoryId, get: GetStatus) -> QueryOptions<Opaque<Uncommitted>> {
    let repository = id.clone();
    QueryOptions::new(
        keys::status(id),
        move |previous: Option<&Opaque<Uncommitted>>, _| {
            let previous = previous.cloned();
            let since = previous.as_ref().map(|status| status.version.clone());
            let reply = get(repository.clone(), since);
            async move {
                match reply.await? {
                    // Only ever the answer when there was a previous status to compare with.
                    StatusReply::Unchanged { .. } => previous.ok_or_else(|| {
                        super::QueryError::msg("The status was unchanged, from none.")
                    }),
                    StatusReply::Status(status) => Ok(Opaque::new(*status)),
                }
            }
            .boxed()
        },
    )
}

/// Where HEAD is, and how many files changed (`selectSummary`).
pub fn select_summary(data: &Opaque<Uncommitted>) -> StatusSummary {
    data.summary()
}

/// The conflicted files, and which of them have no conflict markers left.
#[derive(Debug, Clone, PartialEq)]
pub struct ConflictedFiles {
    pub files: Vec<ChangedFile>,
    pub marker_free: Vec<String>,
}

/// The conflicted files in a status (see [`ConflictedFiles`]).
pub fn select_conflicted(data: &Opaque<Uncommitted>) -> ConflictedFiles {
    ConflictedFiles {
        files: data
            .changes
            .unstaged
            .iter()
            .filter(|file| file.status == FileStatus::Conflicted)
            .cloned()
            .collect(),
        marker_free: data.changes.marker_free.clone(),
    }
}

/// The conflicted file to open first: one with conflict markers left, if any is.
pub fn first_to_resolve(conflicted: &ConflictedFiles) -> Option<&ChangedFile> {
    conflicted
        .files
        .iter()
        .find(|file| !conflicted.marker_free.contains(&file.path))
        .or_else(|| conflicted.files.first())
}

/// The conflicted file to open first (see [`first_to_resolve`]) as the repository `id`'s cached
/// status has it; `None` if none is, or the status isn't there. For once a merge or rebase has
/// stopped at conflicts and the status has reloaded.
pub fn first_conflicted_file(id: &str, cx: &App) -> Option<ChangedFile> {
    let status = QueryClient::get_data::<Opaque<Uncommitted>>(&keys::status(id), cx)?;
    first_to_resolve(&select_conflicted(&status)).cloned()
}

/// The staged and unstaged changes, with their line counts if any: the whole status, shared (see
/// the module's doc). A reload that brings changes replaces it, rather than being merged into it
/// file by file: there can be tens of thousands of files, and their rows are rendered by
/// position, so only the ones in view are updated either way.
pub type UncommittedFiles = Opaque<Uncommitted>;

/// The conflicted files (see [`ConflictedFiles`]), for what's shown of them outside the lists
/// (`useConflictedFiles`).
pub fn conflicted_files_query<V: 'static>(
    id: &RepositoryId,
    cx: &mut Context<V>,
) -> Query<Opaque<Uncommitted>, ConflictedFiles> {
    let options = uncommitted_options(id, git_status(cx));
    Query::with_select(options, select_conflicted, cx)
}

/// Where HEAD is, and how many files changed (`useStatus`).
pub fn status_query<V: 'static>(
    id: &RepositoryId,
    cx: &mut Context<V>,
) -> Query<Opaque<Uncommitted>, StatusSummary> {
    let options = uncommitted_options(id, git_status(cx));
    Query::with_select(options, select_summary, cx)
}

/// [`status_query`], for a repository that isn't watched, like those on the home page (see
/// `super::watch::unwatched`; `useUnwatchedStatus`).
pub fn unwatched_status_query<V: 'static>(
    id: &RepositoryId,
    cx: &mut Context<V>,
) -> Query<Opaque<Uncommitted>, StatusSummary> {
    let options = unwatched(uncommitted_options(id, git_status(cx)));
    Query::with_select(options, select_summary, cx)
}

/// The uncommitted files (see [`UncommittedFiles`]); only loaded while `enabled`
/// (`useUncommittedFiles`).
pub fn uncommitted_files_query<V: 'static>(
    id: &RepositoryId,
    enabled: bool,
    cx: &mut Context<V>,
) -> Query<Opaque<Uncommitted>> {
    let options = uncommitted_options(id, git_status(cx)).enabled(enabled);
    Query::new(options, cx)
}

/// The commit HEAD points at; `None` on a branch without commits yet (`headSha`, which
/// `useHeadSha` derived from the status).
pub fn head_sha(head: &Head) -> Option<&str> {
    match head {
        Head::Branch { sha, .. } | Head::Detached { sha } => Some(sha),
        Head::Unborn { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use futures::FutureExt as _;
    use gitto_git::features::status::schema::{StatusCounts, Unchanged, WorkingTreeFiles};
    use gpui_kit::{AppContext as _, TestAppContext};

    use super::*;

    fn repository_id() -> RepositoryId {
        RepositoryId::parse("01890a5d-ac96-774b-bcce-b302099a8057").unwrap()
    }

    fn status() -> Uncommitted {
        Uncommitted {
            head: Head::Branch {
                name: "main".into(),
                sha: "abc".into(),
            },
            upstream: None,
            ahead: 0,
            behind: 0,
            counts: StatusCounts {
                files: 1,
                staged: 0,
                unstaged: 1,
                conflicted: 0,
            },
            changes: WorkingTreeFiles {
                staged: vec![],
                unstaged: vec![
                    serde_json::from_value(serde_json::json!({
                        "path": "a.txt",
                        "status": "modified",
                        "origPath": null,
                        "additions": 1,
                        "deletions": 0,
                    }))
                    .unwrap(),
                ],
                uncounted: false,
                marker_free: vec![],
            },
            version: "v1".into(),
        }
    }

    /// `rpc.git.status.get`, answering `replies` in turn, and the `since` it was asked with.
    fn mock(replies: Vec<StatusReply>) -> (GetStatus, Rc<RefCell<Vec<Option<String>>>>) {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let replies = Rc::new(RefCell::new(replies.into_iter()));
        let get: GetStatus = {
            let calls = Rc::clone(&calls);
            Rc::new(move |_, since| {
                calls.borrow_mut().push(since);
                let reply = replies.borrow_mut().next().expect("asked once more");
                async move { Ok(reply) }.boxed()
            })
        };
        (get, calls)
    }

    /// A view showing what `select` picks of the status.
    struct Probe<S: 'static> {
        query: Query<Opaque<Uncommitted>, S>,
    }

    fn probe<S: Clone + PartialEq + 'static>(
        get: GetStatus,
        select: impl Fn(&Opaque<Uncommitted>) -> S + 'static,
        cx: &mut TestAppContext,
    ) -> gpui_kit::Entity<Probe<S>> {
        cx.update(QueryClient::init);
        let probe = cx.new(|cx| Probe {
            query: Query::with_select(uncommitted_options(&repository_id(), get), select, cx),
        });
        cx.run_until_parked();
        probe
    }

    #[gpui_kit::test]
    fn are_only_sent_again_when_they_changed(cx: &mut TestAppContext) {
        let (get, calls) = mock(vec![
            StatusReply::Status(Box::new(status())),
            StatusReply::Unchanged {
                unchanged: Unchanged,
            },
        ]);
        let probe = probe(get, Opaque::clone, cx);
        let loaded = probe.read_with(cx, |probe, _| probe.query.data().unwrap());
        assert_eq!(loaded.changes, status().changes);
        let file = &loaded.changes.unstaged[0] as *const ChangedFile;

        let refetch = probe.update(cx, |probe, cx| probe.query.refetch(cx));
        cx.run_until_parked();
        refetch.now_or_never().unwrap();

        assert_eq!(*calls.borrow(), [None, Some("v1".to_owned())]);
        let cached = cx.update(|cx| {
            QueryClient::get_data::<Opaque<Uncommitted>>(&keys::status(&repository_id()), cx)
        });
        assert!(Opaque::ptr_eq(&cached.unwrap(), &loaded));
        let shown = probe.read_with(cx, |probe, _| probe.query.data().unwrap());
        assert!(std::ptr::eq(&shown.changes.unstaged[0], file));
    }

    #[gpui_kit::test]
    fn are_kept_as_they_were_loaded_not_copied_for_each_file(cx: &mut TestAppContext) {
        let (get, _) = mock(vec![StatusReply::Status(Box::new(status()))]);
        let first = probe(get, Opaque::clone, cx);
        let second = cx.new(|cx| Probe {
            query: Query::new(uncommitted_options(&repository_id(), mock(vec![]).0), cx),
        });
        let a = first.read_with(cx, |probe, _| probe.query.data().unwrap());
        let b = second.read_with(cx, |probe, _| probe.query.data().unwrap());
        // Shown twice, loaded once, the same files both times.
        assert!(Opaque::ptr_eq(&a, &b));
    }

    #[gpui_kit::test]
    fn are_left_out_of_the_summary(cx: &mut TestAppContext) {
        let (get, _) = mock(vec![StatusReply::Status(Box::new(status()))]);
        let probe = probe(get, select_summary, cx);
        let summary = probe.read_with(cx, |probe, _| probe.query.data().unwrap());
        assert_eq!(
            summary,
            StatusSummary {
                head: status().head,
                upstream: None,
                ahead: 0,
                behind: 0,
                counts: status().counts,
            }
        );
    }

    #[test]
    fn opens_a_file_with_markers_left_first() {
        let file = |path: &str| -> ChangedFile {
            serde_json::from_value(serde_json::json!({
                "path": path,
                "status": "conflicted",
                "origPath": null,
                "additions": null,
                "deletions": null,
            }))
            .unwrap()
        };
        let conflicted = ConflictedFiles {
            files: vec![file("a.txt"), file("b.txt")],
            marker_free: vec!["a.txt".into()],
        };
        assert_eq!(first_to_resolve(&conflicted).unwrap().path, "b.txt");
        let all_free = ConflictedFiles {
            marker_free: vec!["a.txt".into(), "b.txt".into()],
            ..conflicted
        };
        assert_eq!(first_to_resolve(&all_free).unwrap().path, "a.txt");
    }
}
