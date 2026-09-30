import { os } from "@orpc/server";

import type { GitRepos } from "./repo";

export interface GitContext {
  gitRepos: GitRepos;
}

/** Opens the repository named by the procedure's `repositoryId` input as `context.repo`. */
export const withRepo = os
  .$context<GitContext>()
  .middleware(async ({ context, next }, input: { repositoryId: string }) =>
    next({ context: { repo: await context.gitRepos.open(input.repositoryId) } }),
  );
