import { stat } from "node:fs/promises";

import type { RepositoryService } from "@gitto/repository/server";

import { FolderNotFoundError, RepositoryNotFoundError } from "./errors";
import { runGit, WriteQueue, type RunOptions } from "./runner";

/** A repository on disk, with git commands bound to it. */
export interface Repo {
  readonly path: string;
  /** Runs a command that doesn't change the repository. */
  read(args: string[], options?: RunOptions): Promise<string>;
  /** Runs a command that changes the repository, after any earlier writes to it have finished. */
  write(args: string[], options?: RunOptions): Promise<string>;
  /**
   * Runs a write nobody's waiting for, like housekeeping: like `write`, but it gives way (is
   * cancelled, rejecting with an AbortError) as soon as another write to the repository comes in,
   * and the `onWrite` listeners aren't told about it, as it changes nothing that shows.
   */
  writeInBackground(args: string[]): Promise<string>;
  /** Whether HEAD points at a commit; it doesn't on a branch without commits yet. */
  hasHead(): Promise<boolean>;
  /**
   * Calls `listener` after each write to the repository made through Gitto that succeeded, before
   * the write resolves. Returns a function that stops that.
   */
  onWrite(listener: WriteListener): () => void;
}

type WriteListener = () => Promise<void> | void;

/** Opens the repositories that have been added to Gitto. */
export interface GitRepos {
  open(repositoryId: string): Promise<Repo>;
}

export class GitReposImpl implements GitRepos {
  private readonly writes = new WriteQueue();
  /** Who to tell about writes, by repository path. */
  private readonly writeListeners = new Map<string, Set<WriteListener>>();
  /** Cancels the background writes queued or running for a repository, by its path. */
  private readonly background = new Map<string, Set<AbortController>>();

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
      write: (args, options) => {
        // Whatever was being done in the background can wait: someone is waiting for this.
        for (const controller of this.background.get(path) ?? []) controller.abort();
        return this.writes.run(path, async () => {
          const output = await run(args, options);
          await this.wrote(path);
          return output;
        });
      },
      writeInBackground: (args) => {
        const controller = new AbortController();
        const controllers = this.background.get(path) ?? new Set();
        this.background.set(path, controllers);
        controllers.add(controller);
        const { signal } = controller;
        // Git removes its lock files when it's stopped, so cancelling leaves the repository as is.
        return this.writes
          .run(path, () => (signal.aborted ? Promise.reject(signal.reason) : run(args, { signal })))
          .finally(() => {
            controllers.delete(controller);
            if (controllers.size === 0 && this.background.get(path) === controllers) {
              this.background.delete(path);
            }
          });
      },
      hasHead: () =>
        runGit(path, ["rev-parse", "--verify", "--quiet", "HEAD"]).then(
          () => true,
          () => false,
        ),
      onWrite: (listener) => {
        const listeners = this.writeListeners.get(path) ?? new Set();
        this.writeListeners.set(path, listeners);
        listeners.add(listener);
        return () => {
          listeners.delete(listener);
          if (listeners.size === 0 && this.writeListeners.get(path) === listeners) {
            this.writeListeners.delete(path);
          }
        };
      },
    };
  }

  /** Tells the listeners for `path` about a write; one that fails doesn't fail the write. */
  private async wrote(path: string): Promise<void> {
    const listeners = [...(this.writeListeners.get(path) ?? [])];
    await Promise.all(listeners.map(async (listener) => listener()).map(ignoreFailure));
  }
}

function ignoreFailure(promise: Promise<void>): Promise<void> {
  return promise.catch(() => undefined);
}
