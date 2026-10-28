//! Gitto's SQLite database, which every domain keeps its tables in.
//!
//! - [`Db`]: opens the database, migrated, and runs queries on a thread of its own, off the async
//!   executor.
//! - [`migrations`]: the schema, and how it's brought up to date.
//! - [`database_path`]: where Gitto keeps it.
//!
//! Each domain's crate queries its own tables.

mod db;
pub mod migrations;
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
    /// A newer Gitto migrated the database further than this one knows how to read.
    #[error(
        "the database is at version {version}, newer than this Gitto knows ({known}): update Gitto"
    )]
    TooNew { version: u32, known: u32 },
    /// A table the Electron app shares isn't as either app makes it (see
    /// [`migrations::check_shared_tables`]).
    #[error("the database isn't as Gitto makes it: {0}")]
    Schema(String),
    /// The thread the connection is used on couldn't be started.
    #[error("couldn't start the database's thread: {0}")]
    Thread(std::io::Error),
}
