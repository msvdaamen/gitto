import { AlertDialog } from "@kobalte/core/alert-dialog";
import { cn } from "cn";
import TriangleAlert from "lucide-solid/icons/triangle-alert";

import { Button } from "@/components/ui/button";
import { DIALOG_BOX, DialogPortal } from "@/components/ui/dialog";

/**
 * Asks before marking a file resolved with conflict markers left in it, which is only right when
 * they belong in it, like a test's fixture of them: they're staged as they are. Esc, or clicking
 * outside, cancels.
 */
export function MarkersDialog(props: {
  open: boolean;
  path: string;
  /** How many conflicts the view reads in it; none when its markers can't be read as conflicts. */
  left: number;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const name = () => props.path.slice(props.path.lastIndexOf("/") + 1);
  return (
    <AlertDialog open={props.open} onOpenChange={(open) => !open && props.onCancel()} modal>
      <DialogPortal>
        <AlertDialog.Content class={cn(DIALOG_BOX, "max-w-[440px] p-5")}>
          <div class="flex items-start gap-3">
            <span class="grid size-9 shrink-0 place-items-center rounded-[9px] bg-amber-soft text-amber">
              <TriangleAlert size={18} strokeWidth={1.9} />
            </span>
            <div class="min-w-0">
              <AlertDialog.Title class="m-0 text-[15px] font-[680]">
                Mark {name()} resolved with its conflict markers?
              </AlertDialog.Title>
              <AlertDialog.Description class="m-0 mt-2 text-[12.5px] leading-[1.55] text-text-soft">
                {props.left > 0
                  ? `It still has ${props.left} ${props.left === 1 ? "conflict" : "conflicts"}.`
                  : "It still has conflict markers."}{" "}
                Mark it resolved only if they belong in it, like a test's fixture of them: they're
                staged as they are.
              </AlertDialog.Description>
            </div>
          </div>
          <div class="mt-5 flex justify-end gap-2">
            <Button variant="ghost" onClick={props.onCancel}>
              Cancel
            </Button>
            <Button variant="primary" onClick={props.onConfirm}>
              Mark resolved
            </Button>
          </div>
        </AlertDialog.Content>
      </DialogPortal>
    </AlertDialog>
  );
}
