import { ORPCError, os } from "@orpc/server";

import { isApiFailure } from "./errors";
import type { GitRepos } from "./repo";
import type { GitVersion } from "./version";

export interface GitContext {
  gitRepos: GitRepos;
  gitVersion: GitVersion;
}

/**
 * Opens the repository named by the procedure's `repositoryId` input as `context.repo`, and turns
 * the git package's errors into ones the renderer can show. Nothing runs with a git Gitto doesn't
 * support: its commands would fail partway, or be misread, rather than say why.
 */
export const withRepo = os
  .$context<GitContext>()
  .middleware(async ({ context, next }, input: { repositoryId: string }) => {
    try {
      await context.gitVersion.require();
      return await next({ context: { repo: await context.gitRepos.open(input.repositoryId) } });
    } catch (error) {
      throw toApiError(error);
    }
  });

/**
 * The error as it reaches the renderer: with the code the git package's error says (see `ApiCode`
 * in `errors.ts`). Any other error is a bug, which oRPC reports as such.
 */
function toApiError(error: unknown): unknown {
  if (!isApiFailure(error)) return error;
  const { code, status } = error.api;
  return new ORPCError(code, {
    ...(status === undefined ? {} : { status }),
    message: error.message,
    cause: error,
  });
}
