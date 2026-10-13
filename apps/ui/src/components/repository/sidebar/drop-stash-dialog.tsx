import type { Stash } from "@gitto/git/types";
import Trash from "lucide-solid/icons/trash";

import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { useLingering } from "@/hooks/lingering";

/**
 * Asks before deleting the stash `stash`, whose changes are lost with it; open while there's one.
 * Esc, or clicking outside, cancels.
 */
export function DropStashDialog(props: {
  stash: Stash | undefined;
  onCancel: () => void;
  onDrop: (stash: Stash) => void;
}) {
  const stash = useLingering(() => props.stash);
  return (
    <ConfirmDialog
      open={props.stash !== undefined}
      icon={Trash}
      tone="coral"
      title="Delete stash?"
      description={<>"{stash()?.message}" is deleted, and the changes in it are lost.</>}
      onCancel={props.onCancel}
    >
      <Button variant="ghost" onClick={props.onCancel}>
        Cancel
      </Button>
      <Button variant="primary" onClick={() => props.stash && props.onDrop(props.stash)}>
        Delete
      </Button>
    </ConfirmDialog>
  );
}
