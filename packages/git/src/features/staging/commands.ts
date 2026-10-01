import type { Repo } from "../../core/repo";

// The paths go to git's stdin, NUL-separated, rather than on the command line: "Stage all" in a big
// working tree can list more than a command line holds (32k characters on Windows).
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
