//! The database's schema: the migrations in `crates/db/migrations`, applied in order. SQLite's
//! `user_version` counts those applied, so each runs once.
//!
//! To change the schema, add a migration at the end of [`MIGRATIONS`]: never change one that's
//! shipped, as databases that ran it won't run it again.

use rusqlite::{Connection, TransactionBehavior};

use crate::{Error, Result};

/// Every migration, oldest first: the SQL each runs.
pub const MIGRATIONS: &[&str] = &[include_str!("../migrations/0001_repositories.sql")];

/// Applies the migrations `conn` hasn't run yet, each in a transaction of its own with the
/// `user_version` it brings the database to. Refuses a database migrated further than these go,
/// by a newer Gitto: what it has could mean something else to this one.
///
/// Each transaction is `IMMEDIATE`, taking the write lock before it reads `user_version`: another
/// Gitto starting at the same time waits for it, then reads the version this one left, so two
/// can't both apply a migration.
pub fn migrate(conn: &mut Connection, migrations: &[&str]) -> Result<()> {
    let known = u32::try_from(migrations.len()).unwrap_or(u32::MAX);
    loop {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let applied: u32 = tx.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if applied > known {
            return Err(Error::TooNew {
                version: applied,
                known,
            });
        }
        let Some(sql) = usize::try_from(applied)
            .ok()
            .and_then(|applied| migrations.get(applied))
        else {
            return Ok(());
        };
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", applied + 1)?;
        tx.commit()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(conn: &Connection) -> u32 {
        conn.pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn applies_each_migration_once() {
        let mut conn = Connection::open_in_memory().unwrap();
        let migrations = ["CREATE TABLE `a` (`x` integer);"];
        migrate(&mut conn, &migrations).unwrap();
        assert_eq!(version(&conn), 1);

        let migrations = [migrations[0], "ALTER TABLE `a` ADD `y` integer;"];
        migrate(&mut conn, &migrations).unwrap();
        migrate(&mut conn, &migrations).unwrap();
        assert_eq!(version(&conn), 2);
        conn.execute("INSERT INTO `a` (`x`, `y`) VALUES (1, 2)", [])
            .unwrap();
    }

    #[test]
    fn leaves_a_failed_migration_unapplied() {
        let mut conn = Connection::open_in_memory().unwrap();
        let migrations = ["CREATE TABLE `a` (`x` integer); NOT SQL;"];
        assert!(migrate(&mut conn, &migrations).is_err());
        assert_eq!(version(&conn), 0);
        let tables: i64 = conn
            .query_row("SELECT count(*) FROM sqlite_master", [], |row| row.get(0))
            .unwrap();
        assert_eq!(tables, 0);
    }

    #[test]
    fn refuses_a_database_a_newer_gitto_migrated() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", 5).unwrap();
        let error = migrate(&mut conn, MIGRATIONS).unwrap_err();
        assert!(matches!(error, Error::TooNew { version: 5, .. }), "{error}");
    }

    #[test]
    fn waits_for_a_migration_another_gitto_is_applying() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gitto-app.db");
        let migrations = ["CREATE TABLE `a` (`x` integer);"];
        let other = Connection::open(&path).unwrap();
        other.execute_batch("PRAGMA journal_mode = WAL;").unwrap();
        // The other Gitto has begun applying it, but hasn't finished.
        other
            .execute_batch(
                "BEGIN IMMEDIATE; CREATE TABLE `a` (`x` integer); PRAGMA user_version = 1;",
            )
            .unwrap();
        let this = std::thread::spawn(move || {
            let mut conn = Connection::open(path).unwrap();
            migrate(&mut conn, &migrations).map(|()| version(&conn))
        });
        std::thread::sleep(std::time::Duration::from_millis(200));
        other.execute_batch("COMMIT").unwrap();
        assert_eq!(this.join().unwrap().unwrap(), 1);
    }
}
