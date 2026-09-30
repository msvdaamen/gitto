import { stat } from "node:fs/promises";

import type { RepositoryService } from "@gitto/repository/server";
import { ORPCError } from "@orpc/server";

import { toApiError } from "./errors";
import { runGit, WriteQueue, type RunOptions } from "./runner";

/** A repository on disk, with git commands bound to it. */
export interface Repo {
  readonly path: string;
  /** Runs a command that doesn't change the repository. */
  read(args: string[], options?: RunOptions): Promise<string>;
  /** Runs a command that changes the repository, after any earlier writes to it have finished. */
  write(args: string[], options?: RunOptions): Promise<string>;
  /** Whether HEAD points at a commit; it doesn't on a branch without commits yet. */
  hasHead(): Promise<boolean>;
}

/** Opens the repositories that have been added to Gitto. */
export interface GitRepos {
  open(repositoryId: string): Promise<Repo>;
}

export class GitReposImpl implements GitRepos {
  private readonly writes = new WriteQueue();

  constructor(private readonly repositories: RepositoryService) {}

  async open(repositoryId: string): Promise<Repo> {
    const repository = await this.repositories.getRepository(repositoryId);
    if (!repository) throw new ORPCError("NOT_FOUND", { message: "Repository not found." });

    const path = repository.path;
    // Checked up front: git can't start in a missing folder, and spawn reports that with the same
    // ENOENT as a missing git binary.
    const isFolder = await stat(path).then(
      (stats) => stats.isDirectory(),
      () => false,
    );
    if (!isFolder) {
      throw new ORPCError("NOT_FOUND", { message: `${path} no longer exists.` });
    }

    const run = (args: string[], options?: RunOptions) =>
      runGit(path, args, options).catch((error: unknown) => {
        throw toApiError(error, path);
      });

    return {
      path,
      read: run,
      write: (args, options) => this.writes.run(path, () => run(args, options)),
      hasHead: () =>
        runGit(path, ["rev-parse", "--verify", "--quiet", "HEAD"]).then(
          () => true,
          () => false,
        ),
    };
  }
}
