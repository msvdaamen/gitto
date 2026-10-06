import type { Stash } from "@gitto/git/types";
import { AlertDialog } from "@kobalte/core/alert-dialog";
import { cn } from "cn";
import Trash from "lucide-solid/icons/trash";
import { createMemo } from "solid-js";

import { Button } from "@/components/ui/button";
import { DIALOG_BOX, DialogPortal } from "@/components/ui/dialog";

/**
 * Asks before deleting the stash `stash`, whose changes are lost with it; open while there's one.
 * Esc, or clicking outside, cancels.
 */
export function DropStashDialog(props: {
  stash: Stash | undefined;
  onCancel: () => void;
  onDrop: (stash: Stash) => void;
}) {
  // The last one asked about, still named while the dialog closes.
  const stash = createMemo((last: Stash | undefined) => props.stash ?? last);
  return (
    <AlertDialog
      open={props.stash !== undefined}
      onOpenChange={(open) => !open && props.onCancel()}
      modal
      preventScroll
    >
      <DialogPortal>
        <AlertDialog.Content class={cn(DIALOG_BOX, "max-w-[440px] p-5")}>
          <div class="flex items-start gap-3">
            <span class="grid size-9 shrink-0 place-items-center rounded-[9px] bg-coral-soft text-coral">
              <Trash size={18} strokeWidth={1.9} />
            </span>
            <div class="min-w-0">
              <AlertDialog.Title class="m-0 text-[15px] font-[680]">
                Delete stash?
              </AlertDialog.Title>
              <AlertDialog.Description class="m-0 mt-2 text-[12.5px] leading-[1.55] break-words text-text-soft">
                "{stash()?.message}" is deleted, and the changes in it are lost.
              </AlertDialog.Description>
            </div>
          </div>
          <div class="mt-5 flex justify-end gap-2">
            <Button variant="ghost" onClick={props.onCancel}>
              Cancel
            </Button>
            <Button variant="primary" onClick={() => props.stash && props.onDrop(props.stash)}>
              Delete
            </Button>
          </div>
        </AlertDialog.Content>
      </DialogPortal>
    </AlertDialog>
  );
}
