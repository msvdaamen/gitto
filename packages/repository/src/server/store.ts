import { eq } from "drizzle-orm";
import type { NodeSQLiteDatabase } from "drizzle-orm/node-sqlite";

import type { Repository } from "../types";
import { repositories } from "./schema";

export interface RepositoryStore {
  getAll(): Promise<Repository[]>;
  getByPath(path: string): Promise<Repository | undefined>;
  create(repository: Repository): Promise<void>;
  delete(id: string): Promise<void>;
}

export class RepositoryStoreImpl implements RepositoryStore {
  constructor(private readonly db: NodeSQLiteDatabase) {}

  async getAll(): Promise<Repository[]> {
    return this.db.select().from(repositories);
  }

  async getByPath(path: string): Promise<Repository | undefined> {
    const [repository] = await this.db
      .select()
      .from(repositories)
      .where(eq(repositories.path, path))
      .limit(1);
    return repository;
  }

  async create(repository: Repository): Promise<void> {
    await this.db.insert(repositories).values(repository);
  }

  async delete(id: string): Promise<void> {
    await this.db.delete(repositories).where(eq(repositories.id, id));
  }
}
