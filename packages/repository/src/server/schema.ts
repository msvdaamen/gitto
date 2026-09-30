import { sqliteTable, text } from "drizzle-orm/sqlite-core";

export const repositories = sqliteTable("repositories", {
  id: text().primaryKey(),
  name: text().notNull(),
  path: text().notNull().unique(),
});
