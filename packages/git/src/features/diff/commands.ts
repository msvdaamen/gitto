import type { Repo } from "../../core/repo";
import { parseDiff } from "./parse";
import type { ChangedFile, WorkingTreeFiles } from "./schema";

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

export async function getWorkingTreeFiles(
  repo: Repo,
  signal?: AbortSignal,
): Promise<WorkingTreeFiles> {
  // Without a HEAD commit, `--cached` diffs the index against the empty tree.
  const [staged, unstaged, untracked] = await Promise.all([
    repo.read(["diff", "--cached", "--raw", "--numstat", "-z", "-M"], { signal }),
    repo.read(["diff", "--raw", "--numstat", "-z"], { signal }),
    // `git diff` leaves untracked files out.
    repo.read(["ls-files", "--others", "--exclude-standard", "-z"], { signal }),
  ]);
  return {
    staged: parseDiff(staged),
    unstaged: [
      ...parseDiff(unstaged),
      ...untracked
        .split("\0")
        .filter(Boolean)
        .map<ChangedFile>((path) => ({
          path,
          status: "untracked",
          origPath: null,
          additions: null,
          deletions: null,
        })),
    ],
  };
}
