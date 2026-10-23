//! Gitto's SQLite database, which every domain keeps its tables in. Ported from `packages/db`.
//!
//! The Electron app and this crate share one database file (see [`database_path`]) and one set of
//! migrations: drizzle-kit makes them in `apps/electron/migrations` from the TS schema, and both
//! apply them the same way and record them in the same journal (see [`migrator`]). So a database
//! either app migrated opens in the other, while both ship.
//!
//! - [`Db`]: opens the database, migrated, and runs queries off the async executor.
//! - [`migrator`]: Drizzle's migrator, ported.
//! - [`database_path`]: where the Electron app keeps it.
//!
//! TS's `schema.ts`, which gathered the domains' Drizzle tables for drizzle-kit, has no
//! counterpart: the tables are made by the migrations, and each domain's crate queries its own.

mod db;
pub mod migrator;
mod paths;

pub use rusqlite;

pub use crate::db::Db;
pub use crate::paths::{
    DATABASE_FILE, PRODUCT_NAME, Profile, app_data_dir, database_path, user_data_dir,
};

/// What opening the database returns.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Why the database couldn't be opened or migrated.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// SQLite failed, opening the file or running a statement; said as SQLite says it.
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    /// Drizzle's migrator refused the journal, with its message: see [`migrator::migrate`].
    #[error("{0}")]
    Migrations(String),
}
