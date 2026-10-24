//! What the home page shows of each repository. Ported from `components/home/summaries.ts`.

use std::rc::Rc;

use gitto_git::features::overview::schema::Overview;
use gitto_git::features::status::schema::{StatusSummary, Uncommitted};
use gitto_git::input::RepositoryId;
use gitto_repository::Repository;
use gpui_kit::Context;

use crate::data::overview::overview_query;
use crate::data::status::unwatched_status_query;
use crate::data::{Opaque, Query, QueryError};

/// A repository on the home page, with what's loaded of it so far.
#[derive(Debug, Clone, PartialEq)]
pub struct RepositorySummary {
    pub repository: Repository,
    /// Where HEAD is, and how many files changed; `None` until loaded, or if it couldn't be.
    pub status: Option<StatusSummary>,
    pub overview: Option<Overview>,
    /// Why the repository couldn't be read, e.g. its folder is gone.
    pub error: Option<QueryError>,
    /// Whether its status is loaded, or couldn't be.
    pub settled: bool,
}

/// One repository's queries.
struct Entry {
    repository: Repository,
    status: Query<Opaque<Uncommitted>, StatusSummary>,
    overview: Query<Overview>,
}

/// The status and overview of every repository, kept up to date while the home page shows them
/// (see `watch::unwatched`): its view holds them, and [`sync`](Self::sync)s them with the list of
/// repositories. Each repository shows what it has as it loads, and a reload doesn't take the
/// page away meanwhile (`useRepositorySummaries`).
#[derive(Default)]
pub struct RepositorySummaries {
    entries: Vec<Entry>,
}

impl RepositorySummaries {
    pub fn new() -> Self {
        RepositorySummaries::default()
    }

    /// Follows `repositories`: their queries are kept for the ones still there, made for new ones,
    /// and dropped for those that are gone (`mapArray`).
    pub fn sync<V: 'static>(&mut self, repositories: &[Repository], cx: &mut Context<V>) {
        let mut previous = std::mem::take(&mut self.entries);
        for repository in repositories {
            if let Some(at) = previous
                .iter()
                .position(|entry| entry.repository.id == repository.id)
            {
                let mut entry = previous.swap_remove(at);
                entry.repository = repository.clone();
                self.entries.push(entry);
                continue;
            }
            let Ok(id) = RepositoryId::parse(repository.id.clone()) else {
                continue;
            };
            self.entries.push(Entry {
                repository: repository.clone(),
                status: unwatched_status_query(&id, cx),
                overview: overview_query(&id, cx),
            });
        }
    }

    /// What's loaded of each repository, in the list's order.
    pub fn summaries(&self) -> Rc<[RepositorySummary]> {
        self.entries
            .iter()
            .map(|entry| RepositorySummary {
                repository: entry.repository.clone(),
                status: entry.status.data(),
                overview: entry.overview.data(),
                error: entry.status.error().or_else(|| entry.overview.error()),
                settled: entry.status.data().is_some() || entry.status.is_error(),
            })
            .collect()
    }
}
