import type { ChangedFile, Operation, StatusSummary, UncommittedSide } from "@gitto/git/types";

import { isNestedRepository } from "./changes";
import { operationName } from "./conflicts";

/**
 * Whether a file's changes can be discarded from its list: not a conflicted one's, which are
 * resolved instead, nor a repository inside this one, which git won't delete.
 */
export function canDiscard(file: ChangedFile): boolean {
  return file.status !== "conflicted" && !isNestedRepository(file);
}

/** What discarding a file's changes from its list on `side` loses, to ask before doing it. */
export function discardDescription(file: ChangedFile, side: UncommittedSide): string {
  // A copy's source is another file, which is left as it is.
  const from = file.status === "renamed" && file.origPath;
  if (from) {
    return `${file.path} is deleted, and ${from}, which it was renamed from, is put back${side === "staged" ? " as the last commit has it" : ""}. Its changes are lost.`;
  }
  if (side === "staged") {
    return file.status === "added" || file.status === "copied"
      ? `${file.path} is new since the last commit, so it's deleted, along with its unstaged changes.`
      : `${file.path} goes back to how the last commit has it: its staged and unstaged changes are lost.`;
  }
  if (file.status === "untracked") return `${file.path} isn't tracked, so it's deleted for good.`;
  // Only a file added with `--intent-to-add` is new on the unstaged side.
  if (file.status === "added" || file.status === "copied") {
    return `${file.path} is new, so it's deleted for good.`;
  }
  return `The changes to ${file.path} that aren't staged are lost; its staged ones stay.`;
}

/** What's said of the changes discarding them all kept, as it can't discard them: their paths. */
export function keptMessage(paths: string[]): string {
  const named = paths.slice(0, 3).join(", ");
  const more = paths.length > 3 ? ` and ${paths.length - 3} more` : "";
  return `Kept ${named}${more}: a submodule's or a repository's own changes are discarded in it, and a deleted file isn't put back over what has taken its place.`;
}

/**
 * Why the changes can't all be discarded, if they can't. Not while an operation is under way,
 * which would go on without them: a merge would be committed without the branch's changes.
 */
export function discardAllBlocker(
  status: Pick<StatusSummary, "counts">,
  operation: Operation | null,
): string | undefined {
  if (operation) {
    return `Finish or abort ${operationName(operation)} before discarding all changes.`;
  }
  if (status.counts.conflicted) return "Resolve the conflicts before discarding all changes.";
  if (!status.counts.files) return "No changes to discard.";
  return undefined;
}
