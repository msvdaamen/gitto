//! Embeds the Electron app's migrations, which drizzle-kit makes in `apps/electron/migrations`
//! (see `apps/electron/drizzle.config.ts`), so they're made in one place and a new one is picked up
//! without a change here. Writes `migrations.rs` to `OUT_DIR`: the list `crate::migrator::MIGRATIONS`
//! is, each migration's `migration.sql` with `include_str!`.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::{env, fs};

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("set by cargo"));
    let folder = manifest_dir.join("../../apps/electron/migrations");
    // Cargo looks through a folder for changes, so a migration that's added rebuilds the crate.
    println!("cargo::rerun-if-changed={}", folder.display());

    // Drizzle's migrator refuses the folders of older drizzle-kit versions the same way.
    if folder.join("meta/_journal.json").exists() {
        panic!(
            "We detected that you have old drizzle-kit migration folders. You must upgrade \
             drizzle-kit and run \"drizzle-kit up\""
        );
    }

    // As Drizzle's `readMigrationFiles`: each folder that has a `migration.sql`, by name. Drizzle
    // sorts with `localeCompare`; the names start with the 14-digit timestamp drizzle-kit gives
    // them, which order the same way in both.
    let mut migrations: Vec<(String, PathBuf)> = fs::read_dir(&folder)
        .unwrap_or_else(|error| panic!("can't read {}: {error}", folder.display()))
        .map(|entry| entry.expect("a migration folder entry"))
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let sql = entry.path().join("migration.sql");
            sql.is_file().then_some((name, sql))
        })
        .collect();
    migrations.sort();

    let mut code = String::from("&[\n");
    for (name, sql) in &migrations {
        // Drizzle reads its creation time from the first 14 characters: YYYYMMDDHHMMSS.
        let timestamp = name.get(..14).unwrap_or_default();
        assert!(
            timestamp.len() == 14 && timestamp.bytes().all(|byte| byte.is_ascii_digit()),
            "migration {name} doesn't start with a timestamp (YYYYMMDDHHMMSS)"
        );
        let sql = sql.canonicalize().expect("the migration's path");
        writeln!(
            code,
            "    Migration {{ name: {name:?}, sql: include_str!({:?}) }},",
            sql.to_str().expect("a UTF-8 path to the migration"),
        )
        .expect("writing to a string");
    }
    code.push(']');

    let out = PathBuf::from(env::var_os("OUT_DIR").expect("set by cargo")).join("migrations.rs");
    fs::write(&out, code).unwrap_or_else(|error| panic!("can't write {}: {error}", out.display()));
}
