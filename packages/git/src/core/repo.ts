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
  /** Whether HEAD points at a commit; it doesn't on a branch without commits yet. */
  hasHead(): Promise<boolean>;
  /**
   * Records in the index that files whose timestamps changed, but not their content, are
   * unchanged, so reads stop re-reading them (see `getStatus`). Skipped if a refresh is already
   * running; never rejects, as it's only an optimisation (and fails while another git process
   * holds the index).
   */
  refreshIndex(): Promise<void>;
}

/** Opens the repositories that have been added to Gitto. */
export interface GitRepos {
  open(repositoryId: string): Promise<Repo>;
}

export class GitReposImpl implements GitRepos {
  private readonly writes = new WriteQueue();
  /** Repositories whose index is being refreshed. */
  private readonly refreshing = new Set<string>();

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
    const write = (args: string[], options?: RunOptions) =>
      this.writes.run(path, () => run(args, options));

    return {
      path,
      read: run,
      write,
      hasHead: () =>
        runGit(path, ["rev-parse", "--verify", "--quiet", "HEAD"]).then(
          () => true,
          () => false,
        ),
      refreshIndex: async () => {
        if (this.refreshing.has(path)) return;
        this.refreshing.add(path);
        try {
          // `-q`: files that did change aren't an error.
          await write(["update-index", "-q", "--refresh"]);
        } catch {
          // E.g. the index is locked; the next slow status tries again.
        } finally {
          this.refreshing.delete(path);
        }
      },
    };
  }
}
