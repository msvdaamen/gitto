import { ORPCError, os } from "@orpc/server";

import {
  FolderNotFoundError,
  GitError,
  IndexLockedError,
  NoUpstreamError,
  NotARepositoryError,
  PullInterruptedError,
  RepositoryNotFoundError,
} from "./errors";
import type { GitRepos } from "./repo";

export interface GitContext {
  gitRepos: GitRepos;
}

/**
 * Opens the repository named by the procedure's `repositoryId` input as `context.repo`, and turns
 * the git package's errors into ones the renderer can show.
 */
export const withRepo = os
  .$context<GitContext>()
  .middleware(async ({ context, next }, input: { repositoryId: string }) => {
    try {
      return await next({ context: { repo: await context.gitRepos.open(input.repositoryId) } });
    } catch (error) {
      throw toApiError(error);
    }
  });

/** The one place that decides how each git error reaches the renderer. */
function toApiError(error: unknown): unknown {
  if (
    error instanceof RepositoryNotFoundError ||
    error instanceof FolderNotFoundError ||
    error instanceof NotARepositoryError
  ) {
    return new ORPCError("NOT_FOUND", { message: error.message, cause: error });
  }
  if (error instanceof NoUpstreamError) {
    return new ORPCError("PRECONDITION_FAILED", { message: error.message, cause: error });
  }
  if (error instanceof IndexLockedError || error instanceof PullInterruptedError) {
    return new ORPCError("CONFLICT", { message: error.message, cause: error });
  }
  if (error instanceof GitError) {
    return new ORPCError("INTERNAL_SERVER_ERROR", { message: error.message, cause: error });
  }
  return error;
}
