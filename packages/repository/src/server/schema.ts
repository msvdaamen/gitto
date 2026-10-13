import { integer, sqliteTable, text } from "drizzle-orm/sqlite-core";

export const repositories = sqliteTable("repositories", {
  id: text().primaryKey(),
  name: text().notNull(),
  path: text().notNull().unique(),
  /** Where its tab is among the others', from 0 on the left. */
  position: integer().notNull().default(0),
});
