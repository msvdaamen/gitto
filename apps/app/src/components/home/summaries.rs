//! What the home page shows of each repository. Ported from `components/home/summaries.ts`.

use std::rc::Rc;

use gitto_git::features::overview::schema::Overview;
use gitto_git::features::status::schema::StatusSummary;
use gitto_git::input::RepositoryId;
use gitto_repository::Repository;
use gpui_kit::{App, Context, Entity, Subscription};

use gitto_app_data::data::{Changed, DataError, Part, Repositories};

/// A repository on the home page, with what's loaded of it so far.
#[derive(Debug, Clone, PartialEq)]
pub struct RepositorySummary {
    pub repository: Repository,
    /// Where HEAD is, and how many files changed; `None` until loaded, or if it couldn't be.
    pub status: Option<StatusSummary>,
    pub overview: Option<Overview>,
    /// Why the repository couldn't be read, e.g. its folder is gone.
    pub error: Option<DataError>,
    /// Whether its status is loaded, or couldn't be.
    pub settled: bool,
}

/// One repository, as it's shown.
struct Entry {
    repository: Repository,
    data: Entity<gitto_app_data::data::Repository>,
    _subscription: Subscription,
}

/// The status and overview of every repository, kept up to date while the home page shows them:
/// its view holds them, and [`sync`](Self::sync)s them with the list of repositories. Nothing
/// watches them here, so they're loaded again whenever they're shown, and when the window gets
/// focus. Each repository shows what it has as it loads, and a reload doesn't take the page away
/// meanwhile (`useRepositorySummaries`).
#[derive(Default)]
pub struct RepositorySummaries {
    entries: Vec<Entry>,
}

impl RepositorySummaries {
    pub fn new() -> Self {
        RepositorySummaries::default()
    }

    /// Loads again what's shown of `repository`, which nothing watches here.
    fn reload(repository: &Entity<gitto_app_data::data::Repository>, cx: &mut App) {
        repository.update(cx, |repository, cx| {
            if !repository.status().is_fetching() {
                drop(repository.reload(Part::Status, cx));
            }
            if !repository.overview().is_fetching() {
                drop(repository.reload(Part::Overview, cx));
            }
        });
    }

    /// Follows `repositories`: what's shown of them is kept for the ones still there, opened for new ones,
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
            let data = Repositories::open(&id, cx);
            Self::reload(&data, cx);
            let subscription = cx.subscribe(&data, |_, _, Changed(part), cx| {
                if matches!(part, Part::Status | Part::Overview) {
                    cx.notify();
                }
            });
            self.entries.push(Entry {
                repository: repository.clone(),
                data,
                _subscription: subscription,
            });
        }
    }

    /// Loads again what's shown of every repository: when the window gets focus (see
    /// `AppPart::Focus`).
    pub fn reload_all(&self, cx: &mut App) {
        for entry in &self.entries {
            Self::reload(&entry.data, cx);
        }
    }

    /// What's loaded of each repository, in the list's order.
    pub fn summaries(&self, cx: &App) -> Rc<[RepositorySummary]> {
        self.entries
            .iter()
            .map(|entry| {
                let data = entry.data.read(cx);
                let status = data.status();
                let overview = data.overview();
                RepositorySummary {
                    repository: entry.repository.clone(),
                    status: status.data().map(|status| status.summary()),
                    overview: overview.data().cloned(),
                    error: status.error().or_else(|| overview.error()).cloned(),
                    settled: status.data().is_some() || status.is_error(),
                }
            })
            .collect()
    }
}
