import { ORPCError, os } from "@orpc/server";

import { isApiFailure } from "./errors";
import type { GitRepos } from "./repo";
import type { GitVersion } from "./version";

export interface GitContext {
  gitRepos: GitRepos;
  gitVersion: GitVersion;
}

/**
 * For a procedure that runs git outside of a repository, like a clone: turns the git package's
 * errors into ones the renderer can show (see `withApiErrors`), and runs nothing with a git Gitto
 * doesn't support: its commands would fail partway, or be misread, rather than say why.
 */
export const withGit = os.$context<GitContext>().middleware(({ context, next }) =>
  withApiErrors(async () => {
    await context.gitVersion.require();
    return next();
  }),
);

/**
 * As `withGit`, for a procedure on a repository: opens the one named by its `repositoryId` input as
 * `context.repo`.
 */
export const withRepo = os
  .$context<GitContext>()
  .middleware(({ context, next }, input: { repositoryId: string }) =>
    withApiErrors(async () => {
      await context.gitVersion.require();
      return next({ context: { repo: await context.gitRepos.open(input.repositoryId) } });
    }),
  );

/**
 * What `run` (a middleware's `next`, and what comes before it) resolves to, with the git package's
 * errors turned into ones the renderer can show (see `toApiError`): the ones it rejects with, and
 * for a procedure that emits events (a clone's progress, say), the ones its event iterator throws as
 * it's read, after `run` is done.
 */
async function withApiErrors<T extends { output: unknown }>(run: () => Promise<T>): Promise<T> {
  let result: T;
  try {
    result = await run();
  } catch (error) {
    throw toApiError(error);
  }
  const { output } = result;
  return isAsyncIterator(output) ? { ...result, output: mapErrors(output) } : result;
}

function isAsyncIterator(value: unknown): value is AsyncIterator<unknown, unknown> {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as Partial<AsyncIterator<unknown>>).next === "function" &&
    Symbol.asyncIterator in value
  );
}

/** `iterator`'s events, and what it returns, with its errors turned by `toApiError`. */
async function* mapErrors<T, R>(iterator: AsyncIterator<T, R>): AsyncGenerator<T, R> {
  try {
    // Stopping this (as the renderer does when it's no longer read) stops `iterator` too.
    return yield* { [Symbol.asyncIterator]: () => iterator };
  } catch (error) {
    throw toApiError(error);
  }
}

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
