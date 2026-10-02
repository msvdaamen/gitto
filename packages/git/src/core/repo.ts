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
   * Runs a fetch. Not queued with writes: it waits on the network, which a commit or staging a file
   * shouldn't wait for, and it only updates remote-tracking refs and FETCH_HEAD, which they don't
   * touch. Fetches of the same repository do run one at a time, and without a terminal to ask
   * questions on (see `RunOptions.noTerminal`).
   */
  fetch(args: string[], options?: RunOptions): Promise<string>;
  /**
   * Runs `task`, which runs commands through `run`, as one write: no other write to the repository
   * runs before it's done. For a write that checks the repository before and after.
   */
  exclusive<T>(task: (run: GitCommand) => Promise<T>): Promise<T>;
  /** Whether HEAD points at a commit; it doesn't on a branch without commits yet. */
  hasHead(): Promise<boolean>;
  /** Writes the commit-graph, which speeds up the log, once per run (see `CommitGraphs`). */
  updateCommitGraph(): Promise<void>;
}

export type GitCommand = (args: string[], options?: RunOptions) => Promise<string>;

/** Opens the repositories that have been added to Gitto. */
export interface GitRepos {
  open(repositoryId: string): Promise<Repo>;
}

export class GitReposImpl implements GitRepos {
  private readonly writes = new WriteQueue();
  private readonly fetches = new WriteQueue();
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

    const run: GitCommand = (args, options) => runGit(path, args, options);

    return {
      path,
      read: run,
      write: (args, options) => this.writes.run(path, () => run(args, options)),
      fetch: (args, options) =>
        this.fetches.run(path, () => run(args, { ...options, noTerminal: true })),
      exclusive: (task) => this.writes.run(path, () => task(run)),
      hasHead: () => refExists(run, "HEAD"),
      updateCommitGraph: () => this.commitGraphs.update(path),
    };
  }
}

/** Whether `ref` (e.g. `HEAD`, `MERGE_HEAD`) points at a commit. */
export function refExists(run: GitCommand, ref: string): Promise<boolean> {
  return run(["rev-parse", "--verify", "--quiet", ref]).then(
    () => true,
    () => false,
  );
}
