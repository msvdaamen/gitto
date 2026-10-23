// Makes the databases gitto-db's tests open: gitto.db files as the Electron app leaves them. Run
// with `node crates/db/fixtures/make-fixtures.mjs` (node:sqlite needs Node 22.13 or newer).
//
// The Electron app migrates with drizzle-orm 1.0.0-rc.4's node-sqlite migrator (see
// packages/db/src/index.ts). This replays, through node:sqlite as that driver does, the statements
// it runs and the values it binds: `readMigrationFiles` in drizzle-orm/migrator.js and
// `migrateSync` in drizzle-orm/sqlite-core/async/session.js, down to the SQL's whitespace. Each
// statement is `client.prepare(sql).run(...params)`, as in drizzle-orm/node-sqlite/session.js.

import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { fileURLToPath } from "node:url";

const MIGRATIONS = fileURLToPath(new URL("../../../apps/electron/migrations", import.meta.url));
const FIXTURES = fileURLToPath(new URL(".", import.meta.url));

/** drizzle-orm/migrator.js's `readMigrationFiles`. */
function readMigrationFiles(migrationsFolder) {
  const migrations = readdirSync(migrationsFolder)
    .map((subdir) => ({ path: join(migrationsFolder, subdir, "migration.sql"), name: subdir }))
    .filter((it) => existsSync(it.path));
  migrations.sort((a, b) => a.name.localeCompare(b.name));
  return migrations.map((migration) => {
    const query = readFileSync(migration.path).toString();
    return {
      sql: query.split("--> statement-breakpoint"),
      folderMillis: formatToMillis(migration.name.slice(0, 14)),
      hash: createHash("sha256").update(query).digest("hex"),
      name: migration.name,
    };
  });
}

/** drizzle-orm/migrator.utils.js's `formatToMillis`. */
function formatToMillis(dateStr) {
  return Date.UTC(
    parseInt(dateStr.slice(0, 4), 10),
    parseInt(dateStr.slice(4, 6), 10) - 1,
    parseInt(dateStr.slice(6, 8), 10),
    parseInt(dateStr.slice(8, 10), 10),
    parseInt(dateStr.slice(10, 12), 10),
    parseInt(dateStr.slice(12, 14), 10),
  );
}

/** drizzle-orm/sqlite-core/async/session.js's `migrateSync`, for a journal table of version 1. */
function migrate(client, migrations) {
  const run = (sql, ...params) => client.prepare(sql).run(...params);
  const objects = (sql, ...params) => client.prepare(sql).all(...params);
  const newDb =
    objects("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?", "__drizzle_migrations")
      .length === 0;
  if (newDb) {
    run(`
			CREATE TABLE IF NOT EXISTS "__drizzle_migrations" (
				id INTEGER PRIMARY KEY,
				hash text NOT NULL,
				created_at numeric,
				name text,
				applied_at TEXT
			)`);
  }
  const dbMigrations = objects('SELECT id, hash, created_at, name FROM "__drizzle_migrations"');
  const applied = new Set(dbMigrations.map((m) => m.name).filter((n) => n !== null));
  run("BEGIN");
  try {
    for (const migration of migrations.filter((m) => !applied.has(m.name))) {
      for (const stmt of migration.sql) run(stmt);
      run(
        'INSERT INTO "__drizzle_migrations" ("hash", "created_at", "name", "applied_at") values(?, ?, ?, ?)',
        migration.hash,
        migration.folderMillis,
        migration.name,
        new Date().toISOString(),
      );
    }
    run("COMMIT");
  } catch (error) {
    run("ROLLBACK");
    throw error;
  }
}

/** packages/db/src/index.ts's `createDb`, with only `migrations` in the migrations folder. */
function createDb(path, migrations) {
  for (const suffix of ["", "-wal", "-shm"]) rmSync(`${path}${suffix}`, { force: true });
  const client = new DatabaseSync(path);
  client.exec("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;");
  migrate(client, migrations);
  return client;
}

const migrations = readMigrationFiles(MIGRATIONS);

// From a build with only the first migration, which had no `position`: the second one numbers the
// repositories in the order they were added.
const first = createDb(join(FIXTURES, "first-migration.db"), migrations.slice(0, 1));
const insertFirst = first.prepare(
  'insert into "repositories" ("id", "name", "path") values (?, ?, ?)',
);
insertFirst.run("01999a3c-0000-7000-8000-000000000001", "one", "/home/me/one");
insertFirst.run("01999a3c-0000-7000-8000-000000000002", "two", "/home/me/two");
first.close();

// From a build with both, with repositories added as the repository store adds them.
const both = createDb(join(FIXTURES, "all-migrations.db"), migrations);
const insertBoth = both.prepare(
  'insert into "repositories" ("id", "name", "path", "position") values (?, ?, ?, (SELECT coalesce(max("position"), -1) + 1 FROM "repositories"))',
);
insertBoth.run("01999a3c-0000-7000-8000-000000000001", "one", "/home/me/one");
insertBoth.run("01999a3c-0000-7000-8000-000000000002", "two", "/home/me/two");
both.close();
