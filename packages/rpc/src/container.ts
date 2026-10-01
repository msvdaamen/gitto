import type { Db } from "@gitto/db";
import { GitReposImpl, type GitContext } from "@gitto/git/server";
import {
  RepositoryStoreImpl,
  RepositoryServiceImpl,
  type RepositoryContext,
} from "@gitto/repository/server";
import type { SystemContext } from "@gitto/system/server";

export type AppContext = SystemContext & GitContext & RepositoryContext;

export function createContainer(db: Db, system: SystemContext): AppContext {
  const repoStore = new RepositoryStoreImpl(db);
  const repositoryService = new RepositoryServiceImpl(repoStore);
  const gitRepos = new GitReposImpl(repositoryService);

  return { ...system, repositoryService, gitRepos };
}
