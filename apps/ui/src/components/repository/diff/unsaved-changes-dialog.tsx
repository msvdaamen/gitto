import { AlertDialog } from "@kobalte/core/alert-dialog";
import { cn } from "cn";
import TriangleAlert from "lucide-solid/icons/triangle-alert";

import { Button } from "@/components/ui/button";
import { DIALOG_BOX, DialogPortal } from "@/components/ui/dialog";

/** What to do with edits that couldn't be saved, before going on. */
export type UnsavedChoice = "save" | "discard" | "cancel";

/**
 * Asks what to do with a file's edits that couldn't be saved, before what the user asked for (like
 * closing the file) goes on: save them anyway, drop them, or stay. Esc, or clicking outside, stays.
 */
export function UnsavedChangesDialog(props: {
  open: boolean;
  path: string;
  /** Why they weren't saved. */
  reason: string;
  /** The file changed on disk since: saving overwrites that. */
  changedOnDisk: boolean;
  onChoose: (choice: UnsavedChoice) => void;
}) {
  const name = () => props.path.slice(props.path.lastIndexOf("/") + 1);
  return (
    <AlertDialog
      open={props.open}
      onOpenChange={(open) => !open && props.onChoose("cancel")}
      modal
      preventScroll
    >
      <DialogPortal>
        <AlertDialog.Content class={cn(DIALOG_BOX, "max-w-[440px] p-5")}>
          <div class="flex items-start gap-3">
            <span class="grid size-9 shrink-0 place-items-center rounded-[9px] bg-amber-soft text-amber">
              <TriangleAlert size={18} strokeWidth={1.9} />
            </span>
            <div class="min-w-0">
              <AlertDialog.Title class="m-0 text-[15px] font-[680]">
                Save your changes to {name()}?
              </AlertDialog.Title>
              <AlertDialog.Description class="m-0 mt-2 text-[12.5px] leading-[1.55] text-text-soft">
                {props.reason}{" "}
                {props.changedOnDisk
                  ? "Saving replaces the file with your version."
                  : "Your latest changes aren't saved yet."}
              </AlertDialog.Description>
            </div>
          </div>
          <div class="mt-5 flex justify-end gap-2">
            <Button variant="ghost" onClick={() => props.onChoose("cancel")}>
              Cancel
            </Button>
            <Button onClick={() => props.onChoose("discard")}>Discard</Button>
            <Button variant="primary" onClick={() => props.onChoose("save")}>
              {props.changedOnDisk ? "Overwrite" : "Save"}
            </Button>
          </div>
        </AlertDialog.Content>
      </DialogPortal>
    </AlertDialog>
  );
}
