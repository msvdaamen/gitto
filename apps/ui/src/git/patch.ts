/** What a file's patch, as git gives it, says about its changes. */
export interface PatchSummary {
  /** Git can't compare it as text: "Binary files … differ". */
  binary: boolean;
  additions: number;
  deletions: number;
}

/**
 * Reads `patch`'s lines for what the list of files doesn't always say: an untracked file's, or one
 * of too many to count, has no line counts, and one of those can be binary, or of 100,000 lines.
 * About 1ms for a patch of a megabyte.
 */
export function summarizePatch(patch: string): PatchSummary {
  const body = patch.indexOf("\n@@ ");
  if (body === -1) return { binary: isBinary(patch), additions: 0, deletions: 0 };
  let additions = 0;
  let deletions = 0;
  // Each line of the hunks starts with its kind; `\` is "No newline at end of file".
  for (let start = body + 1; start < patch.length;) {
    const kind = patch.charCodeAt(start);
    if (kind === PLUS) additions++;
    else if (kind === MINUS) deletions++;
    else if (kind === D) {
      // Another file's header: a file that changed type is a deletion and an addition to git.
      const hunks = patch.indexOf("\n@@ ", start);
      if (hunks === -1) break;
      start = hunks + 1;
      continue;
    }
    const end = patch.indexOf("\n", start);
    if (end === -1) break;
    start = end + 1;
  }
  return { binary: false, additions, deletions };
}

const PLUS = "+".charCodeAt(0);
const MINUS = "-".charCodeAt(0);
const D = "d".charCodeAt(0);

/** Whether a patch without hunks is of a binary file. */
function isBinary(header: string): boolean {
  return /^(Binary files .* differ|GIT binary patch)$/m.test(header);
}

/**
 * Names `patch` by what's in it, among other patches of the same file: its header (with the object
 * names of both sides on its `index` line, which git hashes the working tree's side for) and its
 * length.
 */
export function patchVersion(patch: string): string {
  const body = patch.indexOf("\n@@ ");
  return `${patch.length}:${body === -1 ? patch : patch.slice(0, body)}`;
}
