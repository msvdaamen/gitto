/** Environment variables the main process starts the backend process with. */
export const BACKEND_ENV = {
  /** Path of the SQLite database. */
  database: "GITTO_DATABASE",
  /** Folder with the database's migrations. */
  migrations: "GITTO_MIGRATIONS",
} as const;
