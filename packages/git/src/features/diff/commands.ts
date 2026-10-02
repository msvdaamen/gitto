import type { Repo } from "../../core/repo";
import { parseDiff } from "./parse";
import type { ChangedFile, FileChange, LineCounts } from "./schema";

export async function getCommitFiles(
  repo: Repo,
  sha: string,
  signal?: AbortSignal,
): Promise<ChangedFile[]> {
  const output = await repo.read(
    [
      "diff-tree",
      "-r",
      "--root",
      "-M",
      "--raw",
      "--numstat",
      "-z",
      "--no-commit-id",
      "--diff-merges=first-parent",
      sha,
    ],
    { signal },
  );
  return parseDiff(output);
}

/**
 * How many characters of paths one command takes: the command line only holds so many (Windows
 * allows 32k characters, git's own arguments included), and `git diff` can't read paths from stdin.
 */
const MAX_PATHSPEC_LENGTH = 16_000;

/**
 * Line counts of uncommitted changes to `files`: what's staged (compared to HEAD), or what isn't
 * (compared to the index). Only for the files asked for, as git has to diff each one. A renamed
 * file needs its `origPath`, or it's counted as a new file. Files without changes on that side,
 * like untracked ones, are left out.
 */
export async function getLineCounts(
  repo: Repo,
  side: "staged" | "unstaged",
  files: Pick<FileChange, "path" | "origPath">[],
  signal?: AbortSignal,
): Promise<LineCounts[]> {
  // Without a HEAD commit, `--cached` diffs the index against the empty tree.
  const args =
    side === "staged"
      ? ["diff", "--cached", "--raw", "--numstat", "-z", "-M"]
      : ["diff", "--raw", "--numstat", "-z"];
  const outputs = await Promise.all(
    chunks(files).map((chunk) => repo.read([...args, "--", ...chunk], { signal })),
  );
  return outputs.flatMap((output) =>
    parseDiff(output).map(({ path, additions, deletions }) => ({ path, additions, deletions })),
  );
}

/**
 * The files' paths in as few groups as fit on a command line each. A rename's two paths stay in
 * the same group: git only sees it's a rename when it diffs both.
 */
function chunks(files: Pick<FileChange, "path" | "origPath">[]): string[][] {
  const groups: string[][] = [];
  let length = Infinity;
  for (const file of files) {
    const paths = file.origPath ? [file.path, file.origPath] : [file.path];
    // Each path also takes a space and, on Windows, quotes if it has spaces.
    const added = paths.reduce((sum, path) => sum + path.length + 3, 0);
    length += added;
    if (length > MAX_PATHSPEC_LENGTH) {
      groups.push([]);
      length = added;
    }
    groups.at(-1)!.push(...paths);
  }
  return groups;
}
