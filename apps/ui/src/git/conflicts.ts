import type { Conflict, ConflictSide, ConflictSides, Operation } from "@gitto/git/types";

// What a conflicted file is, and what's under way in the repository, in the user's words.

/** What one side of a conflict has at the file's path. */
export type SideKind = "file" | "link" | "submodule";

export function sideKind(side: ConflictSide): SideKind {
  if (side.mode === "120000") return "link";
  if (side.mode === "160000") return "submodule";
  return "file";
}

/**
 * How a conflicted file is resolved: in its text, conflict by conflict, when both sides changed it
 * as a text file; or by keeping one side whole otherwise, e.g. when one deleted it, or it's binary;
 * or not at all, once it isn't conflicted any more.
 */
export type ConflictKind =
  | { kind: "text"; contents: string; version: string }
  | { kind: "sides" }
  | { kind: "resolved" };

export function conflictKind(conflict: Conflict): ConflictKind {
  const { base, ours, theirs, text } = conflict;
  if (!base && !ours && !theirs) return { kind: "resolved" };
  if (ours && theirs && sideKind(ours) === "file" && sideKind(theirs) === "file" && text) {
    return { kind: "text", ...text };
  }
  return { kind: "sides" };
}

/** What each side did to the file, e.g. "Deleted in theirs, changed in ours". */
export function describeConflict({ base, ours, theirs }: ConflictSides): string {
  if (ours && theirs) return base ? "Changed on both sides" : "Added on both sides";
  if (ours) return base ? "Deleted in theirs, changed in ours" : "Added in ours, not in theirs";
  if (theirs) return base ? "Deleted in ours, changed in theirs" : "Added in theirs, not in ours";
  return "Deleted on both sides";
}

/** What a side has, e.g. "A submodule at 1a2b3c4", or "Deleted". */
export function describeSide(side: ConflictSide | null): string {
  if (!side) return "Deleted";
  switch (sideKind(side)) {
    case "link":
      return "A symbolic link";
    case "submodule":
      return `A submodule at ${side.oid.slice(0, 7)}`;
    case "file":
      return side.mode === "100755" ? "An executable file" : "A file";
  }
}

/** The button that keeps a side whole: "Keep ours", or "Delete, as theirs does". */
export function keepLabel(sides: ConflictSides, side: "ours" | "theirs"): string {
  return sides[side] ? `Keep ${side}` : `Delete, as ${side} does`;
}

/** What's under way, e.g. "Merging feature into main", or "Rebasing feature onto main". */
export function operationTitle(operation: Operation): string {
  switch (operation.kind) {
    case "merge":
      return `Merging ${operation.merging}${operation.into ? ` into ${operation.into}` : ""}`;
    case "rebase":
      return `Rebasing ${operation.branch ?? "HEAD"} onto ${operation.onto}`;
    case "cherry-pick":
    case "revert": {
      const verb = operation.kind === "cherry-pick" ? "Cherry-picking" : "Reverting";
      // None between the commits of a series, once one was committed by hand.
      const commit = operation.commit;
      return commit ? `${verb} ${commit.sha} ${commit.subject}`.trimEnd() : `${verb} commits`;
    }
    case "am":
      return "Applying patches";
  }
}

/** How far it's got, e.g. "3/7", or "2 more to go"; `undefined` if that isn't known. */
export function operationProgress(operation: Operation): string | undefined {
  switch (operation.kind) {
    case "rebase":
    case "am":
      return operation.steps ? `${operation.steps.step}/${operation.steps.total}` : undefined;
    case "cherry-pick":
    case "revert":
      return operation.remaining > 0 ? `${operation.remaining} more to go` : undefined;
    case "merge":
      return undefined;
  }
}

/** The operation's name, to ask whether to abort it: "the merge", say. */
export function operationName(operation: Operation): string {
  return operation.kind === "am" ? "applying the patches" : `the ${operation.kind}`;
}
