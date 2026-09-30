import type { Repo } from "../../core/repo";
import { parseDiff } from "./parse";
import type { ChangedFile } from "./schema";

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
): Promise<ChangedFile[]> {
  // Without a HEAD commit there's nothing to diff against but the (empty) tree `--cached` uses.
  const base = (await repo.hasHead()) ? ["HEAD"] : ["--cached"];
  const output = await repo.read(["diff", "--raw", "--numstat", "-z", "-M", ...base], { signal });
  return parseDiff(output);
}
