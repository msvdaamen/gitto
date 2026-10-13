import Trash from "lucide-solid/icons/trash";
import { createMemo } from "solid-js";

import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { branchName } from "@/git/status";

/** A branch, by its short name, with how many commits it has that its remote one doesn't. */
export interface Unpushed {
  name: string;
  ahead: number;
}

/** A branch to delete, and what goes with it, as its menu found them (see `BranchMenu`). */
export type BranchDeletion =
  | {
      /** A local branch, by its full ref name. */
      ref: string;
      kind: "local";
      /** The remote branch it tracks, by its short name, which is kept; `null` if none. */
      upstream: string | null;
      /** How many commits it has that its remote branch doesn't. */
      ahead: number;
    }
  | {
      /** A remote branch, by its full ref name. */
      ref: string;
      kind: "remote";
      /** Whether it's deleted on its remote: not if the remote's gone, its branches left here. */
      onRemote: boolean;
      /** The local branches tracking it, which are deleted with it. */
      tracking: Unpushed[];
    };

/** `names` in a sentence: `a`, `a and b`, `a, b and c`. */
function listed(names: string[]): string {
  return names.length <= 1
    ? (names[0] ?? "")
    : `${names.slice(0, -1).join(", ")} and ${names.at(-1)}`;
}

/** `count` commits, in words: "1 commit", "3 commits". */
function commits(count: number): string {
  return `${count} ${count === 1 ? "commit" : "commits"}`;
}

/** What deleting `deletion` deletes, and keeps, in words. */
function consequences(deletion: BranchDeletion): string {
  const name = branchName(deletion.ref);
  if (deletion.kind === "local") {
    const { upstream, ahead } = deletion;
    const kept = upstream ? ` ${upstream} is kept on the remote.` : "";
    const unpushed =
      upstream && ahead ? ` ${name} has ${commits(ahead)} that ${upstream} doesn't.` : "";
    return `${name} is deleted, with any commits only it has.${kept}${unpushed}`;
  }
  const { onRemote, tracking } = deletion;
  const where = onRemote
    ? `${name} is deleted on the remote, for everyone, with any commits only it has.`
    : `${name} is deleted here: its remote is gone.`;
  const names = tracking.map((local) => local.name);
  const locals =
    names.length === 0
      ? ""
      : names.length === 1
        ? ` So is the local branch ${names[0]}, which tracks it.`
        : ` So are the local branches ${listed(names)}, which track it.`;
  const ahead = tracking.filter((local) => local.ahead > 0);
  const lost =
    ahead.length === 1 && ahead[0]!.ahead === 1
      ? "which is lost unless another branch has it"
      : "which are lost unless another branch has them";
  const unpushed =
    ahead.length === 0
      ? ""
      : ` ${listed(ahead.map((local) => `${local.name} has ${commits(local.ahead)}`))} that ${name} doesn't, ${lost}.`;
  return `${where}${locals}${unpushed}`;
}

/**
 * Asks before deleting a branch, `deletion`, saying what goes with it: for a remote one, the local
 * branches tracking it, and the commits of theirs that were never pushed. Open while there's one.
 * Esc, or clicking outside, cancels.
 */
export function DeleteBranchDialog(props: {
  deletion: BranchDeletion | undefined;
  onCancel: () => void;
  onDelete: (deletion: BranchDeletion) => void;
}) {
  // The last one asked about, still named while the dialog closes.
  const deletion = createMemo((last: BranchDeletion | undefined) => props.deletion ?? last);
  return (
    <ConfirmDialog
      open={props.deletion !== undefined}
      icon={Trash}
      tone="coral"
      title={deletion()?.kind === "remote" ? "Delete remote branch?" : "Delete branch?"}
      description={deletion() ? consequences(deletion()!) : ""}
      onCancel={props.onCancel}
    >
      <Button variant="ghost" onClick={props.onCancel}>
        Cancel
      </Button>
      <Button variant="primary" onClick={() => props.deletion && props.onDelete(props.deletion)}>
        Delete
      </Button>
    </ConfirmDialog>
  );
}
