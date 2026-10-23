//! Applies the Electron app's migrations exactly as its migrator does: drizzle-orm 1.0.0-rc.4's,
//! for node:sqlite (`migrate` from `drizzle-orm/node-sqlite/migrator`), which made the gitto.db
//! files users have. Both apps record what they applied in Drizzle's journal table, the same rows
//! in the same way, so the Electron app opens a database this migrated without reapplying
//! anything, and the other way round.
//!
//! Ported from Drizzle's `readMigrationFiles` (drizzle-orm/migrator.js), `migrateSync`
//! (drizzle-orm/sqlite-core/async/session.js) and the journal table's upgrade
//! (drizzle-orm/up-migrations/sqlite.js), including their SQL, whose text SQLite keeps in its schema.

use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use crate::{Error, Result};

/// Drizzle's journal of the migrations it applied: its default name, which the Electron app keeps.
pub const MIGRATIONS_TABLE: &str = "__drizzle_migrations";

/// What separates a migration's statements: drizzle-kit writes it between them.
const STATEMENT_BREAKPOINT: &str = "--> statement-breakpoint";

/// The journal table as Drizzle makes it, whitespace and all: SQLite keeps the text in its schema.
const CREATE_MIGRATIONS_TABLE: &str = "
\t\t\tCREATE TABLE IF NOT EXISTS \"__drizzle_migrations\" (
\t\t\t\tid INTEGER PRIMARY KEY,
\t\t\t\thash text NOT NULL,
\t\t\t\tcreated_at numeric,
\t\t\t\tname text,
\t\t\t\tapplied_at TEXT
\t\t\t)";

const INSERT_MIGRATION: &str = "INSERT INTO \"__drizzle_migrations\" (\"hash\", \"created_at\", \"name\", \"applied_at\") values(?, ?, ?, ?)";

/// A migration drizzle-kit made: `apps/electron/migrations/<name>/migration.sql`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Migration {
    /// Its folder's name: when it was made (`YYYYMMDDHHMMSS`, UTC), then a name drizzle-kit made up.
    /// What tells Drizzle whether it's been applied.
    pub name: &'static str,
    /// Its `migration.sql`, as is.
    pub sql: &'static str,
}

/// The Electron app's migrations, oldest first: embedded from `apps/electron/migrations` (see
/// `build.rs`), which drizzle-kit makes from the TS schema.
pub static MIGRATIONS: &[Migration] = include!(concat!(env!("OUT_DIR"), "/migrations.rs"));

impl Migration {
    /// Its statements: the SQL split at each `--> statement-breakpoint`, as Drizzle does, without
    /// trimming. Each is one statement (see [`migrate`]).
    pub fn statements(&self) -> impl Iterator<Item = &'static str> {
        self.sql.split(STATEMENT_BREAKPOINT)
    }

    /// The SHA-256 of its SQL, in lowercase hex: the journal's `hash`.
    pub fn hash(&self) -> String {
        Sha256::digest(self.sql.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    /// When it was made, from its name, in milliseconds since the Unix epoch: the journal's
    /// `created_at` (Drizzle's `folderMillis`).
    pub fn created_at(&self) -> i64 {
        let part = |range: std::ops::Range<usize>| -> i64 {
            self.name
                .get(range)
                .and_then(|digits| digits.parse().ok())
                .unwrap_or_default()
        };
        utc_millis(
            part(0..4),
            part(4..6) - 1,
            part(6..8),
            part(8..10),
            part(10..12),
            part(12..14),
        )
    }
}

/// Applies the `migrations` that aren't in the journal, as Drizzle's `migrateSync` does:
///
/// - It makes the journal table if there's none, and upgrades one of an older Drizzle (see
///   [`upgrade_journal`]).
/// - A migration is applied if its name isn't in the journal. Neither the hash nor the time is
///   looked at, so a migration that was edited after it was applied isn't applied again.
/// - The pending ones are applied in one transaction, each followed by its journal row; if any
///   fails, none is.
///
/// Drizzle runs each statement of a migration with node:sqlite's `prepare(sql).run()`, which runs
/// only the first statement in its text. Here a text with more than one is an error instead
/// (drizzle-kit puts a breakpoint between statements, and this crate's tests check every migration
/// is so split), so a migration can't be applied in part.
pub fn migrate(conn: &mut Connection, migrations: &[Migration]) -> Result<()> {
    let mut local: Vec<&Migration> = migrations.iter().collect();

    let exists = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?",
            [MIGRATIONS_TABLE],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if exists {
        let mut columns = conn.prepare("SELECT name as column_name FROM pragma_table_info(?)")?;
        let has_name = columns
            .query_map([MIGRATIONS_TABLE], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .iter()
            .any(|column| column == "name");
        drop(columns);
        // Version 0 of the table has no `name`; version 1, Drizzle's latest, does.
        if !has_name {
            upgrade_journal(conn, &mut local)?;
        }
    } else {
        conn.execute(CREATE_MIGRATIONS_TABLE, [])?;
    }

    let applied: HashSet<String> = {
        let mut rows =
            conn.prepare("SELECT id, hash, created_at, name FROM \"__drizzle_migrations\"")?;
        rows.query_map([], |row| row.get::<_, Value>(3))?
            .filter_map(|name| match name {
                Ok(Value::Text(name)) => Some(Ok(name)),
                Ok(_) => None,
                Err(error) => Some(Err(error)),
            })
            .collect::<rusqlite::Result<_>>()?
    };

    // `BEGIN`, as Drizzle: deferred.
    let tx = conn.transaction()?;
    for migration in local.iter().filter(|m| !applied.contains(m.name)) {
        for statement in migration.statements() {
            run(&tx, statement)?;
        }
        tx.execute(
            INSERT_MIGRATION,
            params![
                migration.hash(),
                migration.created_at(),
                migration.name,
                iso_timestamp(SystemTime::now())
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Runs one statement to its end, whether it returns rows or not, as node:sqlite's `run()` does.
fn run(conn: &Connection, statement: &str) -> rusqlite::Result<()> {
    let mut statement = conn.prepare(statement)?;
    let mut rows = statement.raw_query();
    while rows.next()?.is_some() {}
    Ok(())
}

/// Drizzle's upgrade of a journal table of version 0 (`id`, `hash`, `created_at`), from Drizzle
/// before 1.0, to version 1, which adds `name` and `applied_at`. It names each row after the local
/// migration it matches: by `created_at` to the second, or by hash when the time matches several
/// or none. A row that matches none fails the upgrade, with Drizzle's message.
///
/// Sorts `local` as Drizzle does (its array, in place): by time, then name. They're then applied in
/// that order.
fn upgrade_journal(conn: &mut Connection, local: &mut [&Migration]) -> Result<()> {
    let rows: Vec<(Value, Value, Value)> = {
        let mut rows = conn
            .prepare("SELECT id, hash, created_at FROM \"__drizzle_migrations\" ORDER BY id ASC")?;
        rows.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    local.sort_by(|a, b| a.created_at().cmp(&b.created_at()).then(a.name.cmp(b.name)));

    enum Matched {
        Id(Value),
        Millis(String),
        Hash(Value),
    }
    let mut backfill: Vec<(&str, Matched)> = Vec::new();
    let mut unmatched: Vec<(&Value, &Value)> = Vec::new();
    for (id, hash, created_at) in &rows {
        let stringified = js_string(created_at);
        // Its time to the second: the last three digits (milliseconds) as zeros.
        let cut = stringified
            .char_indices()
            .nth_back(2)
            .map_or(0, |(index, _)| index);
        let millis = js_number(&format!("{}000", &stringified[..cut]));
        let candidates: Vec<&&Migration> = local
            .iter()
            .filter(|m| m.created_at() as f64 == millis)
            .collect();
        let hash_text = match hash {
            Value::Text(hash) => Some(hash.as_str()),
            _ => None,
        };
        let (matched, by_millis) = match candidates.as_slice() {
            [only] => (Some(**only), true),
            [] => (
                // The last with that hash, as Drizzle's map by hash keeps it.
                local
                    .iter()
                    .rev()
                    .find(|m| Some(m.hash().as_str()) == hash_text)
                    .copied(),
                false,
            ),
            several => (
                several
                    .iter()
                    .find(|m| hash_text.is_some_and(|hash| !hash.is_empty() && m.hash() == hash))
                    .map(|m| **m),
                false,
            ),
        };
        match matched {
            Some(migration) => {
                let by = if js_truthy(id) {
                    Matched::Id(id.clone())
                } else if by_millis {
                    Matched::Millis(stringified)
                } else {
                    Matched::Hash(hash.clone())
                };
                backfill.push((migration.name, by));
            }
            None => unmatched.push((id, created_at)),
        }
    }
    if !unmatched.is_empty() {
        let listed: Vec<String> = unmatched
            .iter()
            .map(|(id, created_at)| {
                format!(
                    "[id: {}, created_at: {}]",
                    js_string(id),
                    js_string(created_at)
                )
            })
            .collect();
        return Err(Error::Migrations(format!(
            "While upgrading your database migrations table we found {} ({}) migrations in the \
             database that do not match any local migration. This means that some migrations were \
             applied to the database but are missing from the local environment",
            unmatched.len(),
            listed.join(", ")
        )));
    }

    let tx = conn.transaction()?;
    tx.execute(
        "ALTER TABLE \"__drizzle_migrations\" ADD COLUMN \"name\" text",
        [],
    )?;
    tx.execute(
        "ALTER TABLE \"__drizzle_migrations\" ADD COLUMN \"applied_at\" TEXT",
        [],
    )?;
    for (name, by) in backfill {
        let set = "UPDATE \"__drizzle_migrations\" SET \"name\" = ?, \"applied_at\" = NULL WHERE";
        match by {
            Matched::Id(id) => tx.execute(&format!("{set} \"id\" = ?"), params![name, id])?,
            Matched::Millis(created_at) => tx.execute(
                &format!("{set} \"created_at\" = ?"),
                params![name, created_at],
            )?,
            Matched::Hash(hash) => {
                tx.execute(&format!("{set} \"hash\" = ?"), params![name, hash])?
            }
        };
    }
    tx.commit()?;
    Ok(())
}

/// JS's `String(value)` of a value node:sqlite read: how Drizzle prints and compares one.
fn js_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Integer(integer) => integer.to_string(),
        Value::Real(real) => js_number_string(*real),
        Value::Text(text) => text.clone(),
        // A Uint8Array: its bytes, comma-separated.
        Value::Blob(bytes) => bytes
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(","),
    }
}

/// JS's `String(number)`, for the numbers a journal holds: whole ones as integers.
fn js_number_string(number: f64) -> String {
    if number.is_nan() {
        "NaN".to_owned()
    } else if number.is_infinite() {
        if number > 0.0 {
            "Infinity"
        } else {
            "-Infinity"
        }
        .to_owned()
    } else if number.fract() == 0.0 && number.abs() < 1e21 {
        format!("{number:.0}")
    } else {
        number.to_string()
    }
}

/// JS's `Number(text)`: the number it says, or NaN.
fn js_number(text: &str) -> f64 {
    let text = text.trim();
    if text.is_empty() {
        return 0.0;
    }
    let (sign, unsigned) = match text.as_bytes()[0] {
        b'-' => (-1.0, &text[1..]),
        b'+' => (1.0, &text[1..]),
        _ => (1.0, text),
    };
    if unsigned == "Infinity" {
        return sign * f64::INFINITY;
    }
    for (prefix, radix) in [
        ("0x", 16),
        ("0X", 16),
        ("0o", 8),
        ("0O", 8),
        ("0b", 2),
        ("0B", 2),
    ] {
        if let Some(digits) = text.strip_prefix(prefix) {
            return u64::from_str_radix(digits, radix).map_or(f64::NAN, |n| n as f64);
        }
    }
    // Rust also reads `inf` and `nan`, which JS doesn't.
    if unsigned
        .bytes()
        .any(|byte| byte.is_ascii_alphabetic() && byte != b'e' && byte != b'E')
    {
        return f64::NAN;
    }
    text.parse().unwrap_or(f64::NAN)
}

/// Whether JS takes `value` as true, as Drizzle's `dbRow.id ? … : …` does.
fn js_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Integer(integer) => *integer != 0,
        Value::Real(real) => *real != 0.0 && !real.is_nan(),
        Value::Text(text) => !text.is_empty(),
        Value::Blob(_) => true,
    }
}

/// JS's `Date.UTC(year, month, day, hour, minute, second)`, month from 0: out-of-range parts carry
/// over, and a year from 0 to 99 is 1900 on.
fn utc_millis(year: i64, month: i64, day: i64, hour: i64, minute: i64, second: i64) -> i64 {
    let year = if (0..=99).contains(&year) {
        1900 + year
    } else {
        year
    };
    let year = year + month.div_euclid(12);
    let month = month.rem_euclid(12) + 1;
    let days = days_from_civil(year, month, 1) + day - 1;
    days * 86_400_000 + hour * 3_600_000 + minute * 60_000 + second * 1000
}

/// JS's `new Date().toISOString()` of `time`: `2026-10-13T12:41:37.000Z`. The journal's
/// `applied_at`.
pub(crate) fn iso_timestamp(time: SystemTime) -> String {
    let millis = match time.duration_since(UNIX_EPOCH) {
        Ok(since) => since.as_millis() as i64,
        Err(before) => -(before.duration().as_millis() as i64),
    };
    let days = millis.div_euclid(86_400_000);
    let of_day = millis.rem_euclid(86_400_000);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        of_day / 3_600_000,
        of_day / 60_000 % 60,
        of_day / 1000 % 60,
        of_day % 1000
    )
}

/// Days since 1970-01-01 of a date in the proleptic Gregorian calendar (Howard Hinnant's
/// `days_from_civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The date of a number of days since 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    /// A gitto.db as the Electron app left it, after applying every migration and adding two
    /// repositories (see `fixtures/make-fixtures.mjs`).
    const ALL_MIGRATIONS_FIXTURE: &[u8] = include_bytes!("../fixtures/all-migrations.db");
    /// A gitto.db as a build of the Electron app with only the first migration left it, after
    /// adding two repositories.
    const FIRST_MIGRATION_FIXTURE: &[u8] = include_bytes!("../fixtures/first-migration.db");

    /// A row of the journal, with the type SQLite stored `created_at` as.
    #[derive(Debug, Clone, PartialEq)]
    struct JournalRow {
        id: Value,
        hash: Value,
        created_at: Value,
        created_at_type: String,
        name: Value,
        applied_at: Value,
    }

    fn journal(conn: &Connection) -> Vec<JournalRow> {
        conn.prepare(
            "SELECT id, hash, created_at, typeof(created_at), name, applied_at
             FROM __drizzle_migrations ORDER BY rowid",
        )
        .unwrap()
        .query_map([], |row| {
            Ok(JournalRow {
                id: row.get(0)?,
                hash: row.get(1)?,
                created_at: row.get(2)?,
                created_at_type: row.get(3)?,
                name: row.get(4)?,
                applied_at: row.get(5)?,
            })
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
    }

    /// The journal without when each migration was applied, which differs from run to run.
    fn without_applied_at(rows: Vec<JournalRow>) -> Vec<JournalRow> {
        rows.into_iter()
            .map(|row| JournalRow {
                applied_at: Value::Null,
                ..row
            })
            .collect()
    }

    /// Everything in the database's schema, as SQLite keeps it.
    fn schema(conn: &Connection) -> Vec<(String, String, Option<String>)> {
        conn.prepare("SELECT type, name, sql FROM sqlite_master ORDER BY type, name")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    fn repositories(conn: &Connection) -> Vec<(String, String, String, i64)> {
        conn.prepare("SELECT id, name, path, position FROM repositories ORDER BY rowid")
            .unwrap()
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    /// `fixture`, written to `dir` as gitto.db.
    fn copy_fixture(dir: &Path, fixture: &[u8]) -> PathBuf {
        let path = dir.join("gitto.db");
        std::fs::write(&path, fixture).unwrap();
        path
    }

    /// Whether `text` is as JS's `Date.prototype.toISOString` writes, e.g. 2026-10-13T12:41:37.000Z.
    fn is_iso_timestamp(text: &str) -> bool {
        text.len() == 24
            && text.bytes().enumerate().all(|(index, byte)| match index {
                4 | 7 => byte == b'-',
                10 => byte == b'T',
                13 | 16 => byte == b':',
                19 => byte == b'.',
                23 => byte == b'Z',
                _ => byte.is_ascii_digit(),
            })
    }

    fn text(text: &str) -> Value {
        Value::Text(text.to_owned())
    }

    #[test]
    fn embeds_every_migration_drizzle_kit_made_in_order() {
        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/electron/migrations");
        let mut names: Vec<String> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.join("migration.sql").is_file())
            .map(|path| path.file_name().unwrap().to_str().unwrap().to_owned())
            .collect();
        names.sort();
        let embedded: Vec<&str> = MIGRATIONS.iter().map(|m| m.name).collect();
        assert_eq!(embedded, names);
        for migration in MIGRATIONS {
            let sql = std::fs::read_to_string(folder.join(migration.name).join("migration.sql"));
            assert_eq!(migration.sql, sql.unwrap());
        }
        // In the order Drizzle applies them, by name, which is also by time.
        assert!(MIGRATIONS.windows(2).all(|pair| {
            pair[0].name < pair[1].name && pair[0].created_at() < pair[1].created_at()
        }));
    }

    #[test]
    fn hashes_and_dates_each_migration_as_drizzle_does() {
        let first = MIGRATIONS[0];
        assert_eq!(first.name, "20260930091652_dapper_captain_stacy");
        // `sha256sum migration.sql`, and what Drizzle wrote in the fixture.
        assert_eq!(
            first.hash(),
            "e902e0e3b13542dd5370128a20258cbe5b6cdd889bed9197d205b8ad89647ff6"
        );
        // Date.UTC(2026, 8, 30, 9, 16, 52)
        assert_eq!(first.created_at(), 1_790_759_812_000);
        assert_eq!(MIGRATIONS[1].created_at(), 1_791_895_297_000);
    }

    #[test]
    fn splits_a_migration_at_each_statement_breakpoint_without_trimming() {
        let statements: Vec<&str> = MIGRATIONS[1].statements().collect();
        assert_eq!(statements.len(), 2);
        assert_eq!(
            statements[0],
            "ALTER TABLE `repositories` ADD `position` integer DEFAULT 0 NOT NULL;"
        );
        assert!(statements[1].starts_with("\n-- The tabs keep the order they had"));
        assert!(statements[1].ends_with(";\n"));
    }

    #[test]
    fn creates_a_fresh_database() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn, MIGRATIONS).unwrap();

        let rows = journal(&conn);
        assert_eq!(rows.len(), MIGRATIONS.len());
        for (index, (row, migration)) in rows.iter().zip(MIGRATIONS).enumerate() {
            assert_eq!(row.id, Value::Integer(index as i64 + 1));
            assert_eq!(row.hash, Value::Text(migration.hash()));
            assert_eq!(row.created_at, Value::Integer(migration.created_at()));
            assert_eq!(row.name, text(migration.name));
            assert!(
                matches!(&row.applied_at, Value::Text(at) if is_iso_timestamp(at)),
                "{:?}",
                row.applied_at
            );
        }
        assert!(repositories(&conn).is_empty());
    }

    #[test]
    fn writes_the_journal_rows_drizzle_writes() {
        let dir = tempfile::tempdir().unwrap();
        let drizzle = Connection::open(copy_fixture(dir.path(), ALL_MIGRATIONS_FIXTURE)).unwrap();
        let mut rust = Connection::open_in_memory().unwrap();
        migrate(&mut rust, MIGRATIONS).unwrap();

        let (drizzle_rows, rust_rows) = (journal(&drizzle), journal(&rust));
        // Both write when they applied each migration the same way, as text.
        for row in drizzle_rows.iter().chain(&rust_rows) {
            assert!(matches!(&row.applied_at, Value::Text(at) if is_iso_timestamp(at)));
        }
        // A number stored as an integer, as node:sqlite binds Drizzle's JS number.
        assert_eq!(rust_rows[0].created_at_type, "integer");
        assert_eq!(
            without_applied_at(rust_rows),
            without_applied_at(drizzle_rows)
        );
        // And the same tables, down to the text SQLite keeps of each.
        assert_eq!(schema(&rust), schema(&drizzle));
    }

    #[test]
    fn opens_a_database_with_every_migration_without_reapplying_them() {
        let dir = tempfile::tempdir().unwrap();
        let path = copy_fixture(dir.path(), ALL_MIGRATIONS_FIXTURE);
        let before = {
            let conn = Connection::open(&path).unwrap();
            (journal(&conn), schema(&conn), repositories(&conn))
        };
        assert_eq!(before.0.len(), MIGRATIONS.len());

        let mut conn = Connection::open(&path).unwrap();
        migrate(&mut conn, MIGRATIONS).unwrap();

        assert_eq!((journal(&conn), schema(&conn), repositories(&conn)), before);
        let repository = |id: &str, name: &str, position| {
            (
                id.to_owned(),
                name.to_owned(),
                format!("/home/me/{name}"),
                position,
            )
        };
        assert_eq!(
            repositories(&conn),
            [
                repository("01999a3c-0000-7000-8000-000000000001", "one", 0),
                repository("01999a3c-0000-7000-8000-000000000002", "two", 1),
            ]
        );
    }

    #[test]
    fn applies_the_second_migration_to_a_database_with_only_the_first() {
        let dir = tempfile::tempdir().unwrap();
        let path = copy_fixture(dir.path(), FIRST_MIGRATION_FIXTURE);
        let before = journal(&Connection::open(&path).unwrap());
        assert_eq!(before.len(), 1);

        let mut conn = Connection::open(&path).unwrap();
        migrate(&mut conn, MIGRATIONS).unwrap();

        let after = journal(&conn);
        assert_eq!(after.len(), 2);
        assert_eq!(after[0], before[0]);
        assert_eq!(after[1].id, Value::Integer(2));
        assert_eq!(after[1].hash, Value::Text(MIGRATIONS[1].hash()));
        assert_eq!(
            after[1].created_at,
            Value::Integer(MIGRATIONS[1].created_at())
        );
        assert_eq!(after[1].name, text(MIGRATIONS[1].name));
        // The tabs keep the order the repositories were added in.
        let positions: Vec<i64> = repositories(&conn).iter().map(|repo| repo.3).collect();
        assert_eq!(positions, [0, 1]);
        // As Drizzle leaves it.
        let other = tempfile::tempdir().unwrap();
        let drizzle = Connection::open(copy_fixture(other.path(), ALL_MIGRATIONS_FIXTURE)).unwrap();
        assert_eq!(schema(&conn), schema(&drizzle));
    }

    #[test]
    fn applies_a_migration_by_name_whatever_its_hash() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn, &MIGRATIONS[..1]).unwrap();
        conn.execute("UPDATE __drizzle_migrations SET hash = 'edited'", [])
            .unwrap();

        migrate(&mut conn, MIGRATIONS).unwrap();

        let names: Vec<Value> = journal(&conn).into_iter().map(|row| row.name).collect();
        let expected: Vec<Value> = MIGRATIONS.iter().map(|m| text(m.name)).collect();
        assert_eq!(names, expected);
    }

    #[test]
    fn applies_nothing_when_a_pending_migration_fails() {
        let migrations = [
            Migration {
                name: "20260101000000_good",
                sql: "CREATE TABLE `good` (`id` text);",
            },
            Migration {
                name: "20260102000000_bad",
                sql: "CREATE TABLE `bad` (`id` text);--> statement-breakpoint\nNOT SQL;",
            },
        ];
        let mut conn = Connection::open_in_memory().unwrap();

        let error = migrate(&mut conn, &migrations).unwrap_err();

        assert!(error.to_string().contains("syntax error"), "{error}");
        assert!(journal(&conn).is_empty());
        let tables: Vec<String> = schema(&conn).into_iter().map(|(_, name, _)| name).collect();
        assert_eq!(tables, ["__drizzle_migrations"]);
    }

    #[test]
    fn refuses_a_statement_without_a_breakpoint_after_it() {
        // node:sqlite would run only the first statement; here neither is.
        let migrations = [Migration {
            name: "20260101000000_two_statements",
            sql: "CREATE TABLE `a` (`id` text);\nCREATE TABLE `b` (`id` text);",
        }];
        let mut conn = Connection::open_in_memory().unwrap();

        let error = migrate(&mut conn, &migrations).unwrap_err();

        assert!(
            matches!(error, Error::Sqlite(rusqlite::Error::MultipleStatement)),
            "{error:?}"
        );
        assert!(journal(&conn).is_empty());
    }

    #[test]
    fn runs_a_statement_that_returns_rows() {
        let migrations = [Migration {
            name: "20260101000000_select",
            sql: "SELECT 1;--> statement-breakpoint\nPRAGMA user_version = 3;",
        }];
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn, &migrations).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 3);
    }

    /// The journal as Drizzle before 1.0 made it, with `rows` (`id`, `hash`, `created_at`): its
    /// `id` isn't the rowid, so is null unless given.
    fn journal_of_version_0(conn: &Connection, rows: &[(Option<i64>, String, Value)]) {
        conn.execute_batch(
            "CREATE TABLE \"__drizzle_migrations\" \
             (id SERIAL PRIMARY KEY, hash text NOT NULL, created_at numeric)",
        )
        .unwrap();
        for (id, hash, created_at) in rows {
            conn.execute(
                "INSERT INTO \"__drizzle_migrations\" (id, hash, created_at) VALUES (?, ?, ?)",
                params![id, hash, created_at],
            )
            .unwrap();
        }
    }

    #[test]
    fn upgrades_the_journal_of_an_older_drizzle() {
        let mut conn = Connection::open_in_memory().unwrap();
        run(&conn, MIGRATIONS[0].sql).unwrap();
        // Matched by its time, to the second, whatever its hash.
        let created_at = Value::Integer(MIGRATIONS[0].created_at() + 123);
        journal_of_version_0(&conn, &[(None, "another hash".to_owned(), created_at)]);

        migrate(&mut conn, MIGRATIONS).unwrap();

        let rows = journal(&conn);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, text(MIGRATIONS[0].name));
        assert_eq!(rows[0].hash, text("another hash"));
        assert_eq!(rows[0].applied_at, Value::Null);
        assert_eq!(rows[1].name, text(MIGRATIONS[1].name));
        assert!(matches!(&rows[1].applied_at, Value::Text(at) if is_iso_timestamp(at)));
    }

    #[test]
    fn upgrades_the_journal_of_an_older_drizzle_matching_by_hash_or_id() {
        let mut conn = Connection::open_in_memory().unwrap();
        for statement in MIGRATIONS.iter().flat_map(Migration::statements) {
            run(&conn, statement).unwrap();
        }
        journal_of_version_0(
            &conn,
            &[
                // No time that matches: by its hash.
                (None, MIGRATIONS[0].hash(), Value::Null),
                // With an id, which the row is then updated by.
                (Some(7), MIGRATIONS[1].hash(), text("1")),
            ],
        );

        migrate(&mut conn, MIGRATIONS).unwrap();

        let rows: Vec<(Value, Value)> = journal(&conn)
            .into_iter()
            .map(|row| (row.id, row.name))
            .collect();
        assert_eq!(
            rows,
            [
                (Value::Null, text(MIGRATIONS[0].name)),
                (Value::Integer(7), text(MIGRATIONS[1].name)),
            ]
        );
    }

    #[test]
    fn refuses_to_upgrade_a_journal_with_migrations_it_does_not_have() {
        let mut conn = Connection::open_in_memory().unwrap();
        journal_of_version_0(
            &conn,
            &[
                (
                    None,
                    "unknown".to_owned(),
                    Value::Integer(1_000_000_000_123),
                ),
                (Some(2), "unknown".to_owned(), Value::Null),
            ],
        );

        let error = migrate(&mut conn, MIGRATIONS).unwrap_err();

        assert_eq!(
            error.to_string(),
            "While upgrading your database migrations table we found 2 ([id: null, created_at: \
             1000000000123], [id: 2, created_at: null]) migrations in the database that do not \
             match any local migration. This means that some migrations were applied to the \
             database but are missing from the local environment"
        );
        // Left as it was.
        let columns: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('__drizzle_migrations')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(columns, 3);
    }

    #[test]
    fn computes_dates_as_js_date_utc() {
        assert_eq!(utc_millis(1970, 0, 1, 0, 0, 0), 0);
        assert_eq!(utc_millis(2000, 1, 29, 12, 0, 0), 951_825_600_000);
        // Out-of-range parts carry over: the 13th month is January.
        assert_eq!(
            utc_millis(2025, 12, 1, 0, 0, 0),
            utc_millis(2026, 0, 1, 0, 0, 0)
        );
        // Two-digit years are the 1900s.
        assert_eq!(
            utc_millis(99, 0, 1, 0, 0, 0),
            utc_millis(1999, 0, 1, 0, 0, 0)
        );
    }

    #[test]
    fn writes_times_as_js_to_iso_string() {
        let at = |millis: u64| UNIX_EPOCH + std::time::Duration::from_millis(millis);
        assert_eq!(iso_timestamp(at(0)), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            iso_timestamp(at(1_791_895_297_042)),
            "2026-10-13T12:41:37.042Z"
        );
        assert_eq!(
            iso_timestamp(at(951_825_600_999)),
            "2000-02-29T12:00:00.999Z"
        );
    }

    #[test]
    fn reads_values_as_js_does() {
        assert_eq!(js_string(&Value::Null), "null");
        assert_eq!(js_string(&Value::Integer(12)), "12");
        assert_eq!(
            js_string(&Value::Real(1_790_759_812_000.0)),
            "1790759812000"
        );
        assert_eq!(js_string(&Value::Real(1.5)), "1.5");
        assert_eq!(js_string(&Value::Blob(vec![1, 2])), "1,2");
        assert_eq!(js_number("1790759812000"), 1_790_759_812_000.0);
        assert_eq!(js_number(" 12 "), 12.0);
        assert_eq!(js_number(""), 0.0);
        assert_eq!(js_number("0x10"), 16.0);
        assert!(js_number("n000").is_nan());
        assert!(js_number("inf").is_nan());
        assert_eq!(js_number("-Infinity"), f64::NEG_INFINITY);
        assert!(js_truthy(&Value::Integer(1)));
        assert!(!js_truthy(&Value::Integer(0)));
        assert!(!js_truthy(&Value::Null));
        assert!(!js_truthy(&Value::Text(String::new())));
    }
}
