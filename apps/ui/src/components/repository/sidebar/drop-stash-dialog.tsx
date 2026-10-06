import type { Stash } from "@gitto/git/types";
import Trash from "lucide-solid/icons/trash";

import { ConfirmDialog } from "@/components/ui/confirm-dialog";

/**
 * Asks before deleting the stash `stash`, whose changes are lost with it; open while there's one.
 * Esc, or clicking outside, cancels.
 */
export function DropStashDialog(props: {
  stash: Stash | undefined;
  onCancel: () => void;
  onDrop: (stash: Stash) => void;
}) {
  return (
    <ConfirmDialog
      item={props.stash}
      icon={Trash}
      title="Delete stash?"
      description={(stash) => `"${stash.message}" is deleted, and the changes in it are lost.`}
      confirmLabel="Delete"
      onCancel={props.onCancel}
      onConfirm={props.onDrop}
    />
  );
}
