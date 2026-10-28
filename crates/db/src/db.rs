//! Opening the database.

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use rusqlite::Connection;
use rusqlite::config::DbConfig;

use crate::Result;
use crate::migrations::{MIGRATIONS, check_shared_tables, migrate};

/// Gitto's database: one SQLite connection, migrated. Cheap to clone; clones share the connection.
///
/// SQLite blocks, so [`Db::run`] runs each use of the connection on a thread of `blocking`'s
/// pool, off the async executor (GPUI's, or smol's in tests), one at a time.
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl std::fmt::Debug for Db {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Db").finish_non_exhaustive()
    }
}

impl Db {
    /// Opens (or creates) the SQLite database at `path` and applies any pending migrations (see
    /// [`crate::migrations`]). Its folder has to exist.
    pub fn open(path: impl AsRef<Path>) -> Result<Db> {
        Db::init(Connection::open(path)?, MIGRATIONS)
    }

    /// A database of its own in memory, migrated: for tests.
    pub fn open_in_memory() -> Result<Db> {
        Db::init(Connection::open_in_memory()?, MIGRATIONS)
    }

    fn init(mut conn: Connection, migrations: &[&str]) -> Result<Db> {
        // SQLite takes `"text"` for a string where no column has that name, which hides a
        // misspelt column: it's always an identifier here, so that fails.
        conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DQS_DML, false)?;
        conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DQS_DDL, false)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        migrate(&mut conn, migrations)?;
        check_shared_tables(&mut conn)?;
        Ok(Db {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Runs `f` with the connection, on a thread of its own, and returns what it returns.
    pub async fn run<T, F>(&self, f: F) -> T
    where
        F: FnOnce(&mut Connection) -> T + Send + 'static,
        T: Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        blocking::unblock(move || {
            // A panic in another use leaves the connection usable. A rusqlite `Transaction` it had
            // open was rolled back as it was dropped; one it began with a plain `BEGIN` is still
            // open, so it's rolled back here, and each use starts outside a transaction.
            let mut conn = conn.lock().unwrap_or_else(PoisonError::into_inner);
            if !conn.is_autocommit() {
                // If even this fails, `f`'s first statement says why.
                let _ = conn.execute_batch("ROLLBACK");
            }
            f(&mut conn)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pragma(db: &Db, name: &'static str) -> String {
        smol::block_on(db.run(move |conn| {
            conn.query_row(&format!("PRAGMA {name}"), [], |row| {
                row.get::<_, rusqlite::types::Value>(0)
            })
        }))
        .map(|value| match value {
            rusqlite::types::Value::Text(text) => text,
            rusqlite::types::Value::Integer(integer) => integer.to_string(),
            other => format!("{other:?}"),
        })
        .unwrap()
    }

    #[test]
    fn opens_a_database_in_wal_mode_with_foreign_keys_on() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("gitto.db")).unwrap();
        assert_eq!(pragma(&db, "journal_mode"), "wal");
        assert_eq!(pragma(&db, "foreign_keys"), "1");
    }

    #[test]
    fn creates_the_database_and_its_tables() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gitto.db");
        let db = Db::open(&path).unwrap();
        assert!(path.is_file());
        let tables: Vec<String> = smol::block_on(db.run(|conn| {
            conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")?
                .query_map([], |row| row.get(0))?
                .collect::<rusqlite::Result<_>>()
        }))
        .unwrap();
        assert_eq!(tables, ["repositories"]);
        assert_eq!(pragma(&db, "user_version"), MIGRATIONS.len().to_string());
    }

    #[test]
    fn refuses_a_database_in_a_folder_that_isnt_there() {
        let dir = tempfile::tempdir().unwrap();
        let error = Db::open(dir.path().join("missing/gitto.db")).unwrap_err();
        assert!(
            error.to_string().contains("unable to open database file"),
            "{error}"
        );
    }

    #[test]
    fn takes_double_quoted_text_as_an_identifier() {
        let db = Db::open_in_memory().unwrap();
        let error = smol::block_on(
            db.run(|conn| conn.query_row(r#"SELECT "text""#, [], |row| row.get::<_, String>(0))),
        )
        .unwrap_err();
        assert!(error.to_string().contains("no such column"), "{error}");
    }

    #[test]
    fn refuses_a_migration_with_a_double_quoted_string() {
        let migrations = ["CREATE TABLE `t` (`a` text); INSERT INTO `t` VALUES (\"text\");"];
        let error = Db::init(Connection::open_in_memory().unwrap(), &migrations).unwrap_err();
        assert!(error.to_string().contains("no such column"), "{error}");
    }

    #[test]
    fn opens_a_database_in_memory() {
        let db = Db::open_in_memory().unwrap();
        let count: i64 =
            smol::block_on(db.run(|conn| {
                conn.query_row("SELECT count(*) FROM repositories", [], |row| row.get(0))
            }))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn runs_off_the_calling_thread() {
        let db = Db::open_in_memory().unwrap();
        let caller = std::thread::current().id();
        let runner = smol::block_on(db.run(|_| std::thread::current().id()));
        assert_ne!(runner, caller);
    }

    #[test]
    fn keeps_working_after_a_use_panicked() {
        let db = Db::open_in_memory().unwrap();
        let panicked = std::thread::spawn({
            let db = db.clone();
            move || smol::block_on(db.run(|_| panic!("on purpose")))
        })
        .join();
        assert!(panicked.is_err());
        let one: i64 =
            smol::block_on(db.run(|conn| conn.query_row("SELECT 1", [], |row| row.get(0))))
                .unwrap();
        assert_eq!(one, 1);
    }

    #[test]
    fn opens_the_electron_apps_first_database_adding_the_position() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gitto.db");
        Connection::open(&path)
            .unwrap()
            .execute_batch(include_str!(
                "../../../apps/electron/migrations/20260930091652_dapper_captain_stacy/migration.sql"
            ))
            .unwrap();
        let db = Db::open(&path).unwrap();
        let count: i64 = smol::block_on(db.run(|conn| {
            conn.query_row("SELECT count(`position`) FROM repositories", [], |row| {
                row.get(0)
            })
        }))
        .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn rolls_back_a_transaction_a_use_that_panicked_left_open() {
        let db = Db::open_in_memory().unwrap();
        let panicked = std::thread::spawn({
            let db = db.clone();
            move || {
                smol::block_on(db.run(|conn| {
                    conn.execute_batch(
                        "BEGIN; INSERT INTO repositories (id, name, path) VALUES ('1', 'one', '/one')",
                    )
                    .unwrap();
                    panic!("on purpose")
                }))
            }
        })
        .join();
        assert!(panicked.is_err());
        let (autocommit, count): (bool, i64) = smol::block_on(db.run(|conn| {
            let count = conn.query_row("SELECT count(*) FROM repositories", [], |row| row.get(0));
            (conn.is_autocommit(), count.unwrap())
        }));
        assert!(autocommit);
        assert_eq!(count, 0);
    }
}
