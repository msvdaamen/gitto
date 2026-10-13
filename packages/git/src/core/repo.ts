import { stat } from "node:fs/promises";

import type { RepositoryService } from "@gitto/repository/server";

import { FolderNotFoundError, GitError, RepositoryNotFoundError } from "./errors";
import {
  CommitGraphs,
  IndexRefreshes,
  runGit,
  ShownStatuses,
  WriteQueue,
  type RunOptions,
} from "./runner";

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
   * touch. Fetches of the same repository do run one at a time, and are stopped if Gitto exits.
   */
  fetch(args: string[], options?: RunOptions): Promise<string>;
  /**
   * Runs `task`, which runs commands through `run`, as one fetch (see `fetch`): no other fetch of
   * the repository runs before it's done, e.g. to read the FETCH_HEAD a fetch wrote.
   */
  fetching<T>(task: (run: GitCommand) => Promise<T>): Promise<T>;
  /**
   * Runs `task`, which runs commands through `run`, as one write: no other write to the repository
   * runs before it's done. For a write that checks the repository before and after.
   */
  exclusive<T>(task: (run: GitCommand) => Promise<T>): Promise<T>;
  /**
   * Told how long reading the status took: the index is refreshed if the first one of this run
   * was slow, as after a fresh clone (see `IndexRefreshes`).
   */
  statusTook(ms: number): void;
  /**
   * Whether the log has to be sorted by git: an earlier read, left unsorted, had a commit below one
   * of its parents (see `getLog`). Remembered while the app runs, with `sortLog`.
   */
  sortsLog(): boolean;
  sortLog(): void;
  /** Whether HEAD points at a commit; it doesn't on a branch without commits yet. */
  hasHead(): Promise<boolean>;
  /** Writes the commit-graph, which speeds up sorting the log, once per run (`CommitGraphs`). */
  updateCommitGraph(): Promise<void>;
  /**
   * Remembers the status the UI is being sent, as its `statusSnapshot`: `snapshot` resolves to
   * `undefined` if the UI didn't get it after all (see `ShownStatuses`).
   */
  showStatus(snapshot: Promise<string | undefined>): void;
  /**
   * The snapshot of the status the UI last got, once any that's being read is; `undefined` if it
   * didn't get one.
   */
  shownStatus(): Promise<string | undefined>;
  /**
   * What a feature keeps for the repository while the app runs, by `slot`: made the first time
   * it's asked for, the same thing every time after. E.g. the versions of the files it saved.
   */
  keep<T>(slot: RepoSlot<T>): T;
}

export type GitCommand = (args: string[], options?: RunOptions) => Promise<string>;

/** Names something a feature keeps for each repository (see `Repo.keep`), and makes it. */
export interface RepoSlot<T> {
  readonly make: () => T;
}

export function repoSlot<T>(make: () => T): RepoSlot<T> {
  return { make };
}

/** Opens the repositories that have been added to Gitto. */
export interface GitRepos {
  open(repositoryId: string): Promise<Repo>;
}

/** What's kept for one repository, by its path, while the app runs (see `GitReposImpl.open`). */
class RepoState {
  /** Whether git has to sort its log (see `Repo.sortsLog`). */
  sortsLog = false;
  /** What the features keep for it (see `Repo.keep`). */
  readonly kept = new Map<RepoSlot<unknown>, unknown>();
  /** The repository as the commands get it, made once. */
  repo: Repo | undefined;
}

export class GitReposImpl implements GitRepos {
  private readonly writes = new WriteQueue();
  private readonly fetches = new WriteQueue();
  private readonly commitGraphs = new CommitGraphs();
  private readonly shownStatuses = new ShownStatuses();
  private readonly indexRefreshes = new IndexRefreshes(this.writes);
  /** Each repository's state, by its path: the same path is the same repository, whatever its id. */
  private readonly states = new Map<string, RepoState>();

  constructor(private readonly repositories: RepositoryService) {}

  /**
   * The repository, looked up by its id and checked to be a folder every time: one that's removed
   * from Gitto, or whose folder is gone, is refused with why. What's kept for it is made once.
   */
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

    let state = this.states.get(path);
    if (!state) {
      state = new RepoState();
      this.states.set(path, state);
    }
    return (state.repo ??= this.repoAt(path, state));
  }

  /** The repository at `path`, with `state` kept for it. A plain object: tests make their own from it. */
  private repoAt(path: string, state: RepoState): Repo {
    const run: GitCommand = (args, options) => {
      const result = runGit(path, args, options);
      if (options?.rewritesFiles) {
        // Also after a failure: it may have rewritten some of them all the same.
        const refresh = () => this.indexRefreshes.schedule(path);
        result.then(refresh, refresh);
      }
      return result;
    };
    const fetching = <T>(task: (run: GitCommand) => Promise<T>) =>
      this.fetches.run(path, () =>
        task((args, options) => run(args, { ...options, stopOnExit: true })),
      );

    return {
      path,
      read: run,
      write: (args, options) => this.writes.run(path, () => run(args, options)),
      fetch: (args, options) => fetching((queued) => queued(args, options)),
      fetching,
      exclusive: (task) => this.writes.run(path, () => task(run)),
      statusTook: (ms) => this.indexRefreshes.statusTook(path, ms),
      sortsLog: () => state.sortsLog,
      sortLog: () => void (state.sortsLog = true),
      // Any failure is taken for no HEAD, as callers have always had it.
      hasHead: () => refExists(run, "HEAD").catch(() => false),
      updateCommitGraph: () => this.commitGraphs.update(path),
      showStatus: (snapshot) => this.shownStatuses.set(path, snapshot),
      shownStatus: () => this.shownStatuses.get(path),
      keep: <T>(slot: RepoSlot<T>): T => {
        if (!state.kept.has(slot)) state.kept.set(slot, slot.make());
        return state.kept.get(slot) as T;
      },
    };
  }
}

/**
 * Whether the command `args` exits with 0 rather than 1, for one that answers a yes-or-no question
 * that way (`merge-base --is-ancestor`, say); rejects if it failed otherwise.
 */
export function succeeds(run: GitCommand, args: string[], options?: RunOptions): Promise<boolean> {
  return run(args, options).then(
    () => true,
    (error: unknown) => {
      if (error instanceof GitError && error.exitCode === 1) return false;
      throw error;
    },
  );
}

/** The commit `rev` (e.g. `HEAD`, a branch) points at; `null` if none. Rejects if git couldn't tell. */
export function resolveRef(
  run: GitCommand,
  rev: string,
  options?: RunOptions,
): Promise<string | null> {
  return run(["rev-parse", "--verify", "--quiet", `${rev}^{commit}`], options).then(
    (sha) => sha.trim(),
    (error: unknown) => {
      // Exits with 1, saying nothing, when there's no such commit.
      if (error instanceof GitError && error.exitCode === 1) return null;
      throw error;
    },
  );
}

/** Whether `ref` (e.g. `HEAD`, `MERGE_HEAD`) points at a commit; rejects if git couldn't tell. */
export async function refExists(
  run: GitCommand,
  ref: string,
  options?: RunOptions,
): Promise<boolean> {
  return (await resolveRef(run, ref, options)) !== null;
}

/** Whether the index has unmerged files, as after a merge or pop that conflicted. */
export async function hasConflicts(run: GitCommand): Promise<boolean> {
  return (await run(["ls-files", "--unmerged"])) !== "";
}

/** The checked-out branch's name; `null` when HEAD is detached. */
export async function currentBranch(run: GitCommand, options?: RunOptions): Promise<string | null> {
  let ref: string;
  try {
    // The full name: `--short` would make it `heads/main` if there's also a tag called `main`.
    ref = (await run(["symbolic-ref", "--quiet", "HEAD"], options)).trim();
  } catch (error) {
    if (error instanceof GitError && error.exitCode === 1) return null;
    throw error;
  }
  return ref.startsWith("refs/heads/") ? ref.slice("refs/heads/".length) : null;
}
