import { implement, ORPCError, os as base } from "@orpc/server";

import { RepositoryContract } from "../contract";
import { NotAGitRepositoryError } from "./errors";
import type { RepositoryService } from "./service";

export interface RepositoryContext {
  repositoryService: RepositoryService;
}

/** Turns the repository package's errors into ones the renderer can show. */
const withApiErrors = base.middleware(async ({ next }) => {
  try {
    return await next();
  } catch (error) {
    throw toApiError(error);
  }
});

function toApiError(error: unknown): unknown {
  if (error instanceof NotAGitRepositoryError) {
    return new ORPCError("BAD_REQUEST", { message: error.message, cause: error });
  }
  return error;
}

const os = implement(RepositoryContract).$context<RepositoryContext>().use(withApiErrors);

export const repositoryRouter = os.router({
  list: os.list.handler(({ context }) => context.repositoryService.getRepositories()),
  add: os.add.handler(({ context, input }) => context.repositoryService.addRepository(input.path)),
  remove: os.remove.handler(({ context, input }) =>
    context.repositoryService.removeRepository(input.id),
  ),
});
