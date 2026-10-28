//! The database's schema: the migrations in `crates/db/migrations`, applied in order. SQLite's
//! `user_version` counts those applied, so each runs once.
//!
//! To change the schema, add a migration at the end of [`MIGRATIONS`]: never change one that's
//! shipped, as databases that ran it won't run it again. The Electron app keeps its repositories in
//! the same file while both apps ship (see [`crate::database_path`]), with migrations of its own
//! (Drizzle's, in `apps/electron/migrations`): a change to the `repositories` table goes into both,
//! and [`check_shared_tables`] brings one the other app made up to what this one reads.

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

/// Brings the `repositories` table, once `conn` is migrated, up to what this app reads, when the
/// Electron app made it: the first migration keeps it as it is (`IF NOT EXISTS`). The Electron
/// app's first version had no `position`, which is added as its second migration adds it,
/// numbering the repositories in the order they were added. Fails with `Error::Schema` for a table
/// missing another column, which neither app made.
pub fn check_shared_tables(conn: &mut Connection) -> Result<()> {
    let columns: Vec<String> = conn
        .prepare("SELECT `name` FROM pragma_table_info('repositories')")?
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    if let Some(missing) = ["id", "name", "path"]
        .into_iter()
        .find(|column| !columns.iter().any(|name| name == column))
    {
        return Err(Error::Schema(format!(
            "the repositories table has no {missing} column"
        )));
    }
    if !columns.iter().any(|name| name == "position") {
        let tx = conn.transaction()?;
        tx.execute_batch(
            "ALTER TABLE `repositories` ADD `position` integer DEFAULT 0 NOT NULL;
             UPDATE `repositories` SET `position` = (SELECT count(*) FROM `repositories` AS `earlier` WHERE `earlier`.`rowid` < `repositories`.`rowid`);",
        )?;
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

    /// The columns of `repositories`, in order.
    fn columns(conn: &Connection) -> Vec<String> {
        conn.prepare("SELECT `name` FROM pragma_table_info('repositories')")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    #[test]
    fn adds_the_position_the_electron_apps_first_schema_didnt_have() {
        let mut conn = Connection::open_in_memory().unwrap();
        // `apps/electron/migrations/20260930091652_dapper_captain_stacy`, with two repositories.
        conn.execute_batch(concat!(
            include_str!(
                "../../../apps/electron/migrations/20260930091652_dapper_captain_stacy/migration.sql"
            ),
            "INSERT INTO `repositories` VALUES ('b', 'two', '/two'), ('a', 'one', '/one');"
        ))
        .unwrap();
        migrate(&mut conn, MIGRATIONS).unwrap();
        check_shared_tables(&mut conn).unwrap();
        assert_eq!(columns(&conn), ["id", "name", "path", "position"]);
        let positions: Vec<(String, i64)> = conn
            .prepare("SELECT `id`, `position` FROM `repositories` ORDER BY `position`")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        // In the order they were added.
        assert_eq!(positions, [("b".to_owned(), 0), ("a".to_owned(), 1)]);

        // Once it's there, it's left as it is.
        check_shared_tables(&mut conn).unwrap();
        assert_eq!(columns(&conn), ["id", "name", "path", "position"]);
    }

    #[test]
    fn refuses_a_repositories_table_neither_app_made() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE `repositories` (`id` text PRIMARY KEY, `name` text);")
            .unwrap();
        migrate(&mut conn, MIGRATIONS).unwrap();
        let error = check_shared_tables(&mut conn).unwrap_err();
        assert!(matches!(error, Error::Schema(_)), "{error}");
        assert_eq!(
            error.to_string(),
            "the database isn't as Gitto makes it: the repositories table has no path column"
        );
    }
}
