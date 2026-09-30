import type { Db } from "@gitto/db";
import { GitReposImpl, type GitContext } from "@gitto/git/server";
import {
  RepositoryStoreImpl,
  RepositoryServiceImpl,
  type RepositoryService,
} from "@gitto/repository/server";
import type { SystemContext } from "@gitto/system/server";

export type AppContext = SystemContext &
  GitContext & {
    repoService: RepositoryService;
  };

export function createContainer(db: Db, system: SystemContext): AppContext {
  const repoStore = new RepositoryStoreImpl(db);
  const repoService = new RepositoryServiceImpl(repoStore);
  const gitRepos = new GitReposImpl(repoService);

  return { ...system, repoService, gitRepos };
}
