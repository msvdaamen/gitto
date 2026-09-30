import type { Db } from "@gitto/db";
import {
  RepositoryStoreImpl,
  RepositoryServiceImpl,
  type RepositoryService,
} from "@gitto/repository/server";
import type { SystemContext } from "@gitto/system/server";

export type AppContext = SystemContext & {
  repoService: RepositoryService;
};

export function createContainer(db: Db, system: SystemContext): AppContext {
  const repoStore = new RepositoryStoreImpl(db);
  const repoService = new RepositoryServiceImpl(repoStore);

  return { ...system, repoService };
}
