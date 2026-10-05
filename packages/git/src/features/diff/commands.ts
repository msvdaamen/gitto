import { FileTooLargeError } from "../../core/errors";
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

/**
 * The patch of one file a commit changed, compared to its first parent like `getCommitFiles`, so
 * its lines are the ones counted there. A renamed file needs its old path too, to be found as one.
 * With full object names on its `index` line, which `getBlob` reads the whole file by.
 */
export function getCommitFilePatch(
  repo: Repo,
  sha: string,
  file: { path: string; origPath: string | null },
  signal?: AbortSignal,
): Promise<string> {
  return repo.read(
    [
      "diff-tree",
      "-p",
      "-r",
      "--root",
      "-M",
      "--full-index",
      "--no-commit-id",
      "--diff-merges=first-parent",
      sha,
      "--",
      file.path,
      ...(file.origPath ? [file.origPath] : []),
    ],
    { signal },
  );
}

/**
 * The size of the largest file `getBlob` reads. A change of a line or two can be in a file of any
 * size, a generated one say, and the whole of it is sent to the renderer, kept there and
 * highlighted, which takes seconds for half a megabyte already.
 */
export const MAX_BLOB_BYTES = 5 * 1024 * 1024;

/** The contents of a file, by its object name; rejects with `FileTooLargeError` above `MAX_BLOB_BYTES`. */
export async function getBlob(repo: Repo, oid: string, signal?: AbortSignal): Promise<string> {
  const bytes = Number((await repo.read(["cat-file", "-s", oid], { signal })).trim());
  if (bytes > MAX_BLOB_BYTES) throw new FileTooLargeError(bytes);
  return repo.read(["cat-file", "blob", oid], { signal });
}
