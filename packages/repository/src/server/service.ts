import { stat } from "node:fs/promises";
import { basename, join, resolve } from "node:path";

import { v7 as uuidv7 } from "uuid";

import type { Repository } from "../types";
import { NotAGitRepositoryError, RepositoryListError } from "./errors";
import type { RepositoryStore } from "./store";

export interface RepositoryService {
  getRepositories(): Promise<Repository[]>;
  getRepository(id: string): Promise<Repository | undefined>;
  addRepository(path: string): Promise<Repository>;
  removeRepository(id: string): Promise<void>;
}

export class RepositoryServiceImpl implements RepositoryService {
  constructor(private readonly store: RepositoryStore) {}

  async getRepositories(): Promise<Repository[]> {
    return listed("read", this.store.getAll());
  }

  async getRepository(id: string): Promise<Repository | undefined> {
    return listed("read", this.store.getById(id));
  }

  async addRepository(path: string): Promise<Repository> {
    const repoPath = resolve(path);

    const existing = await listed("read", this.store.getByPath(repoPath));
    if (existing) return existing;

    // `.git` is a directory in regular clones and a file in worktrees and submodules.
    const isRepository = await stat(join(repoPath, ".git")).then(
      () => true,
      () => false,
    );
    if (!isRepository) throw new NotAGitRepositoryError(repoPath);

    const repository: Repository = { id: uuidv7(), name: basename(repoPath), path: repoPath };
    await listed("update", this.store.create(repository));
    return repository;
  }

  async removeRepository(id: string): Promise<void> {
    await listed("update", this.store.delete(id));
  }
}

/** `query` of the list of repositories, rejecting with why it failed in words (see the error). */
function listed<T>(action: "read" | "update", query: Promise<T>): Promise<T> {
  return query.catch((error: unknown) => {
    throw new RepositoryListError(action, error);
  });
}
