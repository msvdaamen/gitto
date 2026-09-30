import { implement } from "@orpc/server";

import { RepositoryContract } from "../contract";
import type { RepositoryService } from "./service";

export { RepositoryServiceImpl, type RepositoryService } from "./service";
export { RepositoryStoreImpl, type RepositoryStore } from "./store";

export interface ReposContext {
  repoService: RepositoryService;
}

const os = implement(RepositoryContract).$context<ReposContext>();

export const repositoryRouter = os.router({
  list: os.list.handler(({ context }) => context.repoService.getRepositories()),
  add: os.add.handler(({ context, input }) => context.repoService.addRepository(input.path)),
  remove: os.remove.handler(({ context, input }) => context.repoService.removeRepository(input.id)),
});
