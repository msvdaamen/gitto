import type { Repo } from "../../core/repo";

// Paths go to git's stdin, NUL-separated, rather than on the command line, which holds only so
// many (32k characters on Windows).
const PATHS_FROM_STDIN = ["--pathspec-from-file=-", "--pathspec-file-nul"];

export async function stage(repo: Repo, paths: string[]): Promise<void> {
  await repo.write(["add", ...PATHS_FROM_STDIN], { stdin: nulSeparated(paths) });
}

export async function unstage(repo: Repo, paths: string[]): Promise<void> {
  // `restore --staged` resets to HEAD, so it can't run before the first commit.
  const args = (await repo.hasHead())
    ? ["restore", "--staged", ...PATHS_FROM_STDIN]
    : ["rm", "--cached", "-r", "-q", ...PATHS_FROM_STDIN];
  await repo.write(args, { stdin: nulSeparated(paths) });
}

function nulSeparated(paths: string[]): string {
  return paths.map((path) => `${path}\0`).join("");
}

export async function stageAll(repo: Repo): Promise<void> {
  await repo.write(["add", "-A"]);
}

export async function unstageAll(repo: Repo): Promise<void> {
  if (!(await repo.hasHead())) {
    // Nothing to unstage isn't an error.
    await repo.write(["rm", "--cached", "-r", "-q", "--ignore-unmatch", "."]);
    return;
  }
  // Resetting the whole index (`restore --staged .`, `reset`) would also undo conflicts, and plain
  // `reset` even abort the merge. So only the staged changes that aren't conflicts are restored:
  // `u` leaves out unmerged paths, and without renames both sides of one are listed. Listed
  // through the write queue, so stages still waiting to run are seen.
  const staged = await repo.write([
    "diff",
    "--cached",
    "--name-only",
    "-z",
    "--no-renames",
    "--diff-filter=u",
  ]);
  if (!staged) return;
  await repo.write(["restore", "--staged", ...PATHS_FROM_STDIN], { stdin: staged });
}
