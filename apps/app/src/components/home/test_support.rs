//! Made-up home page summaries for tests: `summary(id, name).branch("main").build()`.

use gitto_git::features::overview::schema::{Activity, HostedRemote, Overview};
use gitto_git::features::status::schema::{Head, StatusCounts, StatusSummary};
use gitto_repository::Repository;

use super::summaries::RepositorySummary;

/// A repository at `/src/{name}` whose overview has loaded, with no remote and nothing done in it,
/// and whose status hasn't.
pub fn summary(id: &str, name: &str) -> SummaryBuilder {
    SummaryBuilder(RepositorySummary {
        repository: Repository {
            id: id.into(),
            name: name.into(),
            path: format!("/src/{name}"),
        },
        status: None,
        overview: Some(Overview {
            remote: None,
            fetched_at: None,
            committed_at: None,
            activity: Vec::new(),
        }),
        error: None,
        settled: false,
    })
}

pub struct SummaryBuilder(RepositorySummary);

impl SummaryBuilder {
    /// The status as loaded, on `branch`, with nothing changed and no upstream.
    pub fn branch(mut self, branch: &str) -> Self {
        self.0.status = Some(StatusSummary {
            head: Head::Branch {
                name: branch.into(),
                sha: "abc".into(),
            },
            upstream: None,
            ahead: 0,
            behind: 0,
            counts: StatusCounts {
                files: 0,
                staged: 0,
                unstaged: 0,
                conflicted: 0,
            },
        });
        self.0.settled = true;
        self
    }

    /// `files` changed and not staged; on `main` if no branch was given.
    pub fn changes(self, files: u64) -> Self {
        let mut this = self.loaded();
        let counts = &mut this.status_mut().counts;
        counts.files = files;
        counts.unstaged = files;
        this
    }

    /// That many commits `ahead` and `behind` its `upstream` (if it has one); on `main` if no
    /// branch was given.
    pub fn upstream(self, upstream: Option<&str>, ahead: u64, behind: u64) -> Self {
        let mut this = self.loaded();
        let status = this.status_mut();
        status.upstream = upstream.map(str::to_owned);
        status.ahead = ahead;
        status.behind = behind;
        this
    }

    /// Its `origin` at `url`.
    pub fn remote(mut self, url: &str) -> Self {
        self.overview_mut().remote = Some(HostedRemote {
            name: "origin".into(),
            url: url.into(),
        });
        self
    }

    /// Last fetched at `at`, in milliseconds since the epoch.
    pub fn fetched_at(mut self, at: f64) -> Self {
        self.overview_mut().fetched_at = Some(at);
        self
    }

    /// What its reflog has.
    pub fn activity(mut self, activity: Vec<Activity>) -> Self {
        self.overview_mut().activity = activity;
        self
    }

    /// Its overview not loaded yet.
    pub fn without_overview(mut self) -> Self {
        self.0.overview = None;
        self
    }

    /// Its status not loaded yet, though another was.
    pub fn loading(mut self) -> Self {
        self.0.settled = false;
        self
    }

    pub fn build(self) -> RepositorySummary {
        self.0
    }

    fn loaded(self) -> Self {
        if self.0.status.is_some() {
            self
        } else {
            self.branch("main")
        }
    }

    fn status_mut(&mut self) -> &mut StatusSummary {
        self.0.status.as_mut().expect("a status")
    }

    fn overview_mut(&mut self) -> &mut Overview {
        self.0.overview.get_or_insert_with(|| Overview {
            remote: None,
            fetched_at: None,
            committed_at: None,
            activity: Vec::new(),
        })
    }
}
