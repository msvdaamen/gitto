import { stat } from "node:fs/promises";

import type { RepositoryService } from "@gitto/repository/server";

import { FolderNotFoundError, RepositoryNotFoundError } from "./errors";
import { CommitGraphs, runGit, WriteQueue, type RunOptions } from "./runner";

/** A repository on disk, with git commands bound to it. */
export interface Repo {
  readonly path: string;
  /** Runs a command that doesn't change the repository. */
  read(args: string[], options?: RunOptions): Promise<string>;
  /** Runs a command that changes the repository, after any earlier writes to it have finished. */
  write(args: string[], options?: RunOptions): Promise<string>;
  /**
   * Runs commands that change the repository as one write: after any earlier writes, and with no
   * other write in between. They're run with `run`; `write` would wait for the task itself.
   */
  writeTogether<T>(
    task: (run: (args: string[], options?: RunOptions) => Promise<string>) => Promise<T>,
  ): Promise<T>;
  /** Whether HEAD points at a commit; it doesn't on a branch without commits yet. */
  hasHead(): Promise<boolean>;
  /** Writes the commit-graph, which speeds up the log, once per run (see `CommitGraphs`). */
  updateCommitGraph(): Promise<void>;
}

/** Opens the repositories that have been added to Gitto. */
export interface GitRepos {
  open(repositoryId: string): Promise<Repo>;
}

export class GitReposImpl implements GitRepos {
  private readonly writes = new WriteQueue();
  private readonly commitGraphs = new CommitGraphs();

  constructor(private readonly repositories: RepositoryService) {}

  async open(repositoryId: string): Promise<Repo> {
    const repository = await this.repositories.getRepository(repositoryId);
    if (!repository) throw new RepositoryNotFoundError(repositoryId);

    const path = repository.path;
    // Checked up front: git can't start in a missing folder, and spawn reports that with the same
    // ENOENT as a missing git binary.
    const isFolder = await stat(path).then(
      (stats) => stats.isDirectory(),
      () => false,
    );
    if (!isFolder) throw new FolderNotFoundError(path);

    const run = (args: string[], options?: RunOptions) => runGit(path, args, options);

    return {
      path,
      read: run,
      write: (args, options) => this.writes.run(path, () => run(args, options)),
      writeTogether: (task) => this.writes.run(path, () => task(run)),
      hasHead: () =>
        runGit(path, ["rev-parse", "--verify", "--quiet", "HEAD"]).then(
          () => true,
          () => false,
        ),
      updateCommitGraph: () => this.commitGraphs.update(path),
    };
  }
}
