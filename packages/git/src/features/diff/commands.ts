import { ChangesTooLargeError, FileTooLargeError, GitError } from "../../core/errors";
import type { GitCommand, Repo } from "../../core/repo";
import { MAX_BLOB_BYTES } from "./limits";
import { parseDiff } from "./parse";
import type { ChangedFile } from "./schema";
import { checkWorkingTreePath } from "./working-tree";

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
  return readPatch(
    repo.read,
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
      ...filePaths(file),
    ],
    signal,
  );
}

/**
 * The patch of one file's unstaged changes: the working tree compared to the index, as in the list
 * of unstaged files. The `index` line names the working tree's side by what git would store it as,
 * which isn't in the repository: `readWorkingTreeFile` reads it whole. An untracked file is shown
 * as added, in full.
 */
export function getUnstagedFilePatch(
  repo: Repo,
  file: { path: string; origPath: string | null; untracked: boolean },
  signal?: AbortSignal,
): Promise<string> {
  if (file.untracked) return getUntrackedFilePatch(repo, file.path, signal);
  return readPatch(
    repo.read,
    ["diff-files", "-p", "-M", "--full-index", "--", ...filePaths(file)],
    signal,
  );
}

/**
 * An untracked file, compared to nothing. Git only compares files outside the index with porcelain
 * `diff --no-index`, which reads the user's diff settings (see `PORCELAIN_DIFF`). It reads any path
 * it's given, so only one inside the working tree is: a link there is read as where it points.
 */
async function getUntrackedFilePatch(
  repo: Repo,
  path: string,
  signal?: AbortSignal,
): Promise<string> {
  await checkWorkingTreePath(repo, path);
  const args = [
    "diff",
    "--no-index",
    "--full-index",
    ...PORCELAIN_DIFF.flags,
    "--",
    "/dev/null",
    path,
  ];
  try {
    return await readPatch(repo.read, args, signal, PORCELAIN_DIFF.config);
  } catch (error) {
    // Exits with 1 when the files differ, which they always do, but also when it can't read one,
    // like a folder: the patch tells them apart.
    if (error instanceof GitError && error.exitCode === 1 && error.stdout) return error.stdout;
    throw error;
  }
}

/**
 * The patch of one file's staged changes: the index compared to HEAD, as in the list of staged
 * files, or to nothing before the first commit. Both sides are in the repository, by the object
 * names on the `index` line.
 */
export async function getStagedFilePatch(
  repo: Repo,
  file: { path: string; origPath: string | null },
  signal?: AbortSignal,
): Promise<string> {
  const diff = (base: string) =>
    readPatch(
      repo.read,
      ["diff-index", "-p", "--cached", "-M", "--full-index", base, "--", ...filePaths(file)],
      signal,
    );
  try {
    return await diff("HEAD");
  } catch (error) {
    // Asked only once it failed: there's a HEAD all but before the first commit.
    if (!(error instanceof GitError) || (await repo.hasHead())) throw error;
    return diff(await emptyTree(repo.read));
  }
}

/** The object name of a tree without files, in the repository's hash. */
async function emptyTree(run: GitCommand): Promise<string> {
  return (await run(["hash-object", "-t", "tree", "--stdin"], { stdin: "" })).trim();
}

/** A file's path, and its previous one if it was renamed: a rename is only found with both. */
function filePaths(file: { path: string; origPath: string | null }): string[] {
  return file.origPath ? [file.path, file.origPath] : [file.path];
}

/**
 * The size of the largest patch sent to the renderer, which parses and highlights all of it. The UI
 * asks before showing a change of many lines, but can't tell how many there are in a file without
 * line counts, like an untracked one: a log of hundreds of megabytes, say.
 */
export const MAX_PATCH_BYTES = 10 * 1024 * 1024;

/**
 * Runs a diff command for one file's patch, through `run`, with settings `config` (as with
 * `git -c`); stopped past `MAX_PATCH_BYTES`.
 */
export function readPatch(
  run: GitCommand,
  args: string[],
  signal?: AbortSignal,
  config?: string[],
): Promise<string> {
  return run(args, {
    signal,
    config,
    maxOutput: { bytes: MAX_PATCH_BYTES, error: () => new ChangesTooLargeError(MAX_PATCH_BYTES) },
  });
}

/**
 * Undoes the user's diff settings that would change a patch from a porcelain command, which reads
 * them (plumbing doesn't): other prefixes than `a/` and `b/`, an external diff tool, a textconv
 * filter. The prefixes are settings rather than `--src-prefix` and `--dst-prefix`, which `git stash
 * show` mangles, and `--default-prefix` is newer than the git Gitto needs.
 */
export const PORCELAIN_DIFF = {
  config: [
    "diff.noprefix=false",
    "diff.mnemonicPrefix=false",
    "diff.srcPrefix=a/",
    "diff.dstPrefix=b/",
  ],
  flags: ["--no-color", "--no-ext-diff", "--no-textconv"],
};

/** The contents of a file, by its object name; rejects with `FileTooLargeError` above `MAX_BLOB_BYTES`. */
export async function getBlob(repo: Repo, oid: string, signal?: AbortSignal): Promise<string> {
  const bytes = Number((await repo.read(["cat-file", "-s", oid], { signal })).trim());
  if (bytes > MAX_BLOB_BYTES) throw new FileTooLargeError(bytes);
  return repo.read(["cat-file", "blob", oid], { signal });
}
