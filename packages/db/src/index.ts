import { defineRelations } from "drizzle-orm";
import { drizzle } from "drizzle-orm/node-sqlite";
import { migrate } from "drizzle-orm/node-sqlite/migrator";

import * as schema from "./schema";

const relations = defineRelations(schema);

/** Opens (or creates) the SQLite database at `path` and applies any pending migrations. */
export function createDb(path: string, migrationsFolder: string) {
  // node:sqlite is built into Electron's Node, so there's no native module to rebuild.
  const db = drizzle({ connection: path, relations });
  db.$client.exec("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;");
  migrate(db, { migrationsFolder });
  return db;
}

export type Db = ReturnType<typeof createDb>;
