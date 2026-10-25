//! What the UI works with, started at launch (`registerRpc` and `createContainer` in
//! `apps/electron/src/main.ts`): the database, the list of repositories, git, the system, and what
//! the UI remembers. Held in one GPUI global, [`Services`], which every view reads:
//!
//! ```ignore
//! let git = Services::global(cx).git().clone();
//! ```
//!
//! There's no RPC: the UI calls the crates directly, off the main thread (see `crate::data`).

pub mod logger;
pub mod paths;
pub mod storage;
pub mod system;

use std::rc::Rc;

use anyhow::Context as _;
use gitto_db::Db;
use gitto_git::GitApi;
use gitto_repository::{RepositoryService, RepositoryStore};
use gpui_kit::{App, Global};

pub use self::paths::AppPaths;
pub use self::storage::LocalStorage;
pub use self::system::SystemContext;
use crate::data::repositories::RepositorySource;

/// The services the UI works with (see the module's doc). Cheap to clone.
#[derive(Clone)]
pub struct Services {
    paths: AppPaths,
    db: Db,
    repositories: RepositoryService,
    /// The list of repositories as the UI reads and changes it: `repositories`, or a test's.
    repository_source: Rc<dyn RepositorySource>,
    git: GitApi,
    system: SystemContext,
    storage: Rc<LocalStorage>,
}

impl Global for Services {}

impl Services {
    /// Opens the database in `paths`' folder, creating the folder first, and starts the services
    /// on it. Git's bugs (failures Gitto doesn't expect) are reported to the log, where the
    /// Electron app sent them to Sentry.
    pub fn start(paths: AppPaths) -> anyhow::Result<Self> {
        std::fs::create_dir_all(paths.user_data())
            .with_context(|| format!("Couldn't create {}", paths.user_data().display()))?;
        let db = Db::open(paths.database())
            .with_context(|| format!("Couldn't open {}", paths.database().display()))?;
        let system = SystemContext::stubs(paths.settings(), paths.last_seen_version());
        let storage = Rc::new(LocalStorage::open(paths.local_storage()));
        Ok(Services::with(paths, db, system, storage))
    }

    /// The services on `db`, with `system` and `storage`: for tests, an in-memory database.
    pub fn with(paths: AppPaths, db: Db, system: SystemContext, storage: Rc<LocalStorage>) -> Self {
        let repositories = RepositoryService::new(RepositoryStore::new(db.clone()));
        let git = GitApi::new(repositories.clone()).on_bug(|error, action| {
            log::error!(
                "[bug] {action} failed ({}): {}",
                error.code().code(),
                error.error()
            );
        });
        Services {
            paths,
            db,
            repository_source: Rc::new(repositories.clone()),
            repositories,
            git,
            system,
            storage,
        }
    }

    /// Services on an in-memory database, with what the UI remembers kept in memory, and the
    /// system's stubs keeping their files in `dir`.
    pub fn in_memory(dir: &std::path::Path) -> anyhow::Result<Self> {
        let paths = AppPaths::new(dir);
        let db = Db::open_in_memory()?;
        let system = SystemContext::stubs(paths.settings(), paths.last_seen_version());
        Ok(Services::with(
            paths,
            db,
            system,
            Rc::new(LocalStorage::in_memory()),
        ))
    }

    /// These services, with git run through `git`: e.g. one checking a git version a test made
    /// up (`GitApi::with`).
    pub fn with_git(mut self, git: GitApi) -> Self {
        self.git = git;
        self
    }

    /// These services, with the list of repositories kept by `source`: e.g. a test's.
    pub fn with_repository_source(mut self, source: Rc<dyn RepositorySource>) -> Self {
        self.repository_source = source;
        self
    }

    /// These services, with `system`: e.g. a test's updates.
    pub fn with_system(mut self, system: SystemContext) -> Self {
        self.system = system;
        self
    }

    pub fn global(cx: &App) -> &Services {
        cx.global::<Services>()
    }

    pub fn paths(&self) -> &AppPaths {
        &self.paths
    }

    pub fn db(&self) -> &Db {
        &self.db
    }

    /// The repositories added to Gitto, in the order of their tabs.
    pub fn repositories(&self) -> &RepositoryService {
        &self.repositories
    }

    /// The list of repositories as the UI reads and changes it (see `data::repositories`).
    pub fn repository_source(&self) -> &Rc<dyn RepositorySource> {
        &self.repository_source
    }

    /// Git, for the repositories added to Gitto (see `GitApi::repo`).
    pub fn git(&self) -> &GitApi {
        &self.git
    }

    pub fn system(&self) -> &SystemContext {
        &self.system
    }

    /// What the UI remembers across restarts, as the renderer's `localStorage` did.
    pub fn storage(&self) -> &Rc<LocalStorage> {
        &self.storage
    }
}
