//! The database's schema: the migrations in `crates/db/migrations`, applied in order. SQLite's
//! `user_version` counts those applied, so each runs once.
//!
//! To change the schema, add a migration at the end of [`MIGRATIONS`]: never change one that's
//! shipped, as databases that ran it won't run it again.

use rusqlite::Connection;

use crate::{Error, Result};

/// Every migration, oldest first: the SQL each runs.
pub const MIGRATIONS: &[&str] = &[include_str!("../migrations/0001_repositories.sql")];

/// Applies the migrations `conn` hasn't run yet, each in a transaction of its own with the
/// `user_version` it brings the database to. Refuses a database migrated further than these go,
/// by a newer Gitto: what it has could mean something else to this one.
pub fn migrate(conn: &mut Connection, migrations: &[&str]) -> Result<()> {
    let applied: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let known = migrations.len() as u32;
    if applied > known {
        return Err(Error::TooNew {
            version: applied,
            known,
        });
    }
    for (version, sql) in (1..).zip(migrations).skip(applied as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
    }
    Ok(())
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
    fn opens_the_electron_apps_database_with_its_repositories() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE `repositories` (`id` text PRIMARY KEY, `name` text NOT NULL, \
             `path` text NOT NULL UNIQUE, `position` integer DEFAULT 0 NOT NULL);
             INSERT INTO `repositories` VALUES ('1', 'one', '/one', 0);",
        )
        .unwrap();
        migrate(&mut conn, MIGRATIONS).unwrap();
        let count: i64 = conn
            .query_row("SELECT count(*) FROM `repositories`", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
}
