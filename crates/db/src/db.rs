//! Opening the database.

use std::any::Any;
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use rusqlite::Connection;
use rusqlite::config::DbConfig;

use crate::Result;
use crate::migrations::{MIGRATIONS, migrate};

/// A use of the connection, sent to its thread (see [`Db::run`]).
type Job = Box<dyn FnOnce(&mut Connection) + Send>;

/// Gitto's database: one SQLite connection, migrated. Cheap to clone; clones share the connection.
///
/// SQLite blocks, so the connection lives on a thread of its own, which runs each use of it that
/// [`Db::run`] sends, one at a time, in the order they're sent, off the async executor (GPUI's, or
/// smol's in tests). The thread ends once every clone is dropped, closing the connection.
#[derive(Clone)]
pub struct Db {
    jobs: mpsc::Sender<Job>,
}

impl std::fmt::Debug for Db {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Db").finish_non_exhaustive()
    }
}

impl Db {
    /// Opens (or creates) the SQLite database at `path` and applies any pending migrations (see
    /// [`crate::migrations`]). Its folder has to exist.
    ///
    /// Unlike [`Db::run`], this blocks the caller while SQLite opens and migrates the file: it's
    /// for starting up, before there's anything to show.
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
        // Another Gitto running at the same time can hold the write lock for a moment: wait for
        // it, rather than fail with "database is locked". rusqlite's default, said here so it's
        // not left to it.
        conn.busy_timeout(Duration::from_secs(5))?;
        // `NORMAL` is enough with WAL: a power cut can lose the last commits, but can't corrupt the
        // file, and a commit doesn't wait for the disk.
        conn.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON;",
        )?;
        migrate(&mut conn, migrations)?;
        let (jobs, received) = mpsc::channel::<Job>();
        thread::Builder::new()
            .name("gitto-db".to_owned())
            .spawn(move || {
                for job in received {
                    job(&mut conn);
                }
            })
            .map_err(crate::Error::Thread)?;
        Ok(Db { jobs })
    }

    /// Runs `f` with the connection, on its thread, and returns what it returns. A panic in `f`
    /// panics here, as if `f` had run here, and leaves the connection to the next use.
    ///
    /// Each use starts outside a transaction: one `f` began and left open, with a plain `BEGIN`,
    /// is rolled back once it returns, as a rusqlite `Transaction` is when it's dropped. That loses
    /// what it wrote, so short of a panic, it's a bug: it panics here too in a debug build.
    pub async fn run<T, F>(&self, f: F) -> T
    where
        F: FnOnce(&mut Connection) -> T + Send + 'static,
        T: Send + 'static,
    {
        let (done, outcome) = async_channel::bounded(1);
        let job: Job = Box::new(move |conn| {
            let ran = panic::catch_unwind(AssertUnwindSafe(|| f(conn)));
            let left_open = !conn.is_autocommit();
            if left_open {
                // If even this fails, the next use's first statement says why.
                let _ = conn.execute_batch("ROLLBACK");
            }
            let ran = match ran {
                Ok(_) if left_open && cfg!(debug_assertions) => Err(Box::new(
                    "a use of the database left a transaction open, so it was rolled back",
                )
                    as Box<dyn Any + Send>),
                ran => ran,
            };
            // The caller may have stopped waiting.
            let _ = done.try_send(ran);
        });
        self.jobs
            .send(job)
            .expect("the database's thread runs as long as a Db is there");
        match outcome.recv().await {
            Ok(Ok(value)) => value,
            Ok(Err(panicked)) => panic::resume_unwind(panicked),
            Err(_) => unreachable!("the database's thread runs every use it's sent"),
        }
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
        let db = Db::open(dir.path().join("gitto-app.db")).unwrap();
        assert_eq!(pragma(&db, "journal_mode"), "wal");
        assert_eq!(pragma(&db, "synchronous"), "1");
        assert_eq!(pragma(&db, "foreign_keys"), "1");
        assert_eq!(pragma(&db, "busy_timeout"), "5000");
    }

    #[test]
    fn creates_the_database_and_its_tables() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gitto-app.db");
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
        let error = Db::open(dir.path().join("missing/gitto-app.db")).unwrap_err();
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
    fn rolls_back_a_transaction_a_use_left_open_and_says_so_in_a_debug_build() {
        let db = Db::open_in_memory().unwrap();
        let left_open = std::thread::spawn({
            let db = db.clone();
            move || {
                smol::block_on(db.run(|conn| {
                    conn.execute_batch(
                        "BEGIN; INSERT INTO repositories (id, name, path) VALUES ('1', 'one', '/one')",
                    )
                    .unwrap();
                }))
            }
        })
        .join();
        if cfg!(debug_assertions) {
            let panicked = left_open.unwrap_err();
            assert_eq!(
                panicked.downcast_ref::<&str>(),
                Some(&"a use of the database left a transaction open, so it was rolled back")
            );
        } else {
            left_open.unwrap();
        }
        let (autocommit, count): (bool, i64) = smol::block_on(db.run(|conn| {
            let count = conn.query_row("SELECT count(*) FROM repositories", [], |row| row.get(0));
            (conn.is_autocommit(), count.unwrap())
        }));
        assert!(autocommit);
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
