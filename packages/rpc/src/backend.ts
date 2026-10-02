import type { Db } from "@gitto/db";
import { GitReposImpl, gitRouter, type GitContext } from "@gitto/git/server";
import {
  repositoryRouter,
  RepositoryServiceImpl,
  RepositoryStoreImpl,
  type RepositoryContext,
} from "@gitto/repository/server";
import { implement } from "@orpc/server";

import { backendContract } from "./contract";
import { createRpcHandler } from "./handler";

export type BackendContext = GitContext & RepositoryContext;

export function createBackendContainer(db: Db): BackendContext {
  const repoStore = new RepositoryStoreImpl(db);
  const repositoryService = new RepositoryServiceImpl(repoStore);
  const gitRepos = new GitReposImpl(repositoryService);

  return { repositoryService, gitRepos };
}

const router = implement(backendContract).$context<BackendContext>().router({
  repository: repositoryRouter,
  git: gitRouter,
});

/** The backend process's side of the API (see `backendContract`). */
export const createBackendRpcHandler = () => createRpcHandler(router);
