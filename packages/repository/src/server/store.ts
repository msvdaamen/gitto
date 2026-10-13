import { asc, eq, sql } from "drizzle-orm";
import type { NodeSQLiteDatabase } from "drizzle-orm/node-sqlite";

import type { Repository } from "../types";
import { repositories } from "./schema";

export interface RepositoryStore {
  getAll(): Promise<Repository[]>;
  getById(id: string): Promise<Repository | undefined>;
  getByPath(path: string): Promise<Repository | undefined>;
  /** Adds `repository` after the others. */
  create(repository: Repository): Promise<void>;
  delete(id: string): Promise<void>;
  /** Puts the repositories in the order of `ids`; ones missing from it keep their place after. */
  reorder(ids: string[]): Promise<void>;
}

/** A repository's columns, without where it's listed. */
const columns = { id: repositories.id, name: repositories.name, path: repositories.path };

export class RepositoryStoreImpl implements RepositoryStore {
  constructor(private readonly db: NodeSQLiteDatabase) {}

  async getAll(): Promise<Repository[]> {
    return this.db.select(columns).from(repositories).orderBy(asc(repositories.position));
  }

  async getById(id: string): Promise<Repository | undefined> {
    const [repository] = await this.db
      .select(columns)
      .from(repositories)
      .where(eq(repositories.id, id))
      .limit(1);
    return repository;
  }

  async getByPath(path: string): Promise<Repository | undefined> {
    const [repository] = await this.db
      .select(columns)
      .from(repositories)
      .where(eq(repositories.path, path))
      .limit(1);
    return repository;
  }

  async create(repository: Repository): Promise<void> {
    await this.db.insert(repositories).values({
      ...repository,
      position: sql`(SELECT coalesce(max(${repositories.position}), -1) + 1 FROM ${repositories})`,
    });
  }

  async delete(id: string): Promise<void> {
    await this.db.delete(repositories).where(eq(repositories.id, id));
  }

  async reorder(ids: string[]): Promise<void> {
    // Past the end of `ids`, so any repository missing from it lands after the ones in it.
    const rest = ids.length;
    this.db.transaction((tx) => {
      tx.update(repositories)
        .set({ position: sql`${repositories.position} + ${rest}` })
        .run();
      ids.forEach((id, position) =>
        tx.update(repositories).set({ position }).where(eq(repositories.id, id)).run(),
      );
    });
  }
}
