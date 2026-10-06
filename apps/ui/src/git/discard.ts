import type { ChangedFile, StatusSummary, UncommittedSide } from "@gitto/git/types";

import { isNestedRepository } from "./changes";

/**
 * Whether a file's changes can be discarded from its list: not a conflicted one's, which are
 * resolved instead, nor a repository inside this one, which git won't delete.
 */
export function canDiscard(file: ChangedFile): boolean {
  return file.status !== "conflicted" && !isNestedRepository(file);
}

/** What discarding a file's changes from its list on `side` loses, to ask before doing it. */
export function discardDescription(file: ChangedFile, side: UncommittedSide): string {
  if (side === "staged") {
    return file.status === "added"
      ? `${file.path} is new since the last commit, so it's deleted, along with its unstaged changes.`
      : `${file.path} goes back to how the last commit has it: its staged and unstaged changes are lost.`;
  }
  if (file.status === "untracked") return `${file.path} isn't tracked, so it's deleted for good.`;
  return `The changes to ${file.path} that aren't staged are lost; its staged ones stay.`;
}

/** Why the changes can't all be discarded, if they can't. */
export function discardAllBlocker(status: Pick<StatusSummary, "counts">): string | undefined {
  if (status.counts.conflicted) return "Resolve the conflicts before discarding all changes.";
  if (!status.counts.files) return "No changes to discard.";
  return undefined;
}
