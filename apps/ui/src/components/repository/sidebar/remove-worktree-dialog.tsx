import type { Worktree } from "@gitto/git/types";
import FolderX from "lucide-solid/icons/folder-x";
import { createMemo } from "solid-js";

import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { branchName } from "@/git/status";

/**
 * Asks before removing the worktree `worktree`, whose folder is deleted with any uncommitted
 * changes in it; open while there's one. Esc, or clicking outside, cancels.
 */
export function RemoveWorktreeDialog(props: {
  worktree: Worktree | undefined;
  onCancel: () => void;
  onRemove: (worktree: Worktree) => void;
}) {
  // The last one asked about, still named while the dialog closes.
  const worktree = createMemo((last: Worktree | undefined) => props.worktree ?? last);
  const branch = () => {
    const ref = worktree()?.branch;
    return ref ? ` The branch ${branchName(ref)} is kept.` : "";
  };
  return (
    <ConfirmDialog
      open={props.worktree !== undefined}
      icon={FolderX}
      tone="coral"
      title="Remove worktree?"
      description={
        <>
          The folder {worktree()?.path} is deleted, and any uncommitted changes in it are lost.
          {branch()}
        </>
      }
      onCancel={props.onCancel}
    >
      <Button variant="ghost" onClick={props.onCancel}>
        Cancel
      </Button>
      <Button variant="primary" onClick={() => props.worktree && props.onRemove(props.worktree)}>
        Remove
      </Button>
    </ConfirmDialog>
  );
}
