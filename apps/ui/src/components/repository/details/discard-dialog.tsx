import { AlertDialog } from "@kobalte/core/alert-dialog";
import { cn } from "cn";
import Undo2 from "lucide-solid/icons/undo-2";
import { createMemo } from "solid-js";

import { Button } from "@/components/ui/button";
import { DIALOG_BOX, DialogPortal } from "@/components/ui/dialog";
import type { DiscardTarget } from "@/git/queries/staging";

/**
 * Asks before discarding changes, `target`, which are lost; open while there are some to ask
 * about. Esc, or clicking outside, cancels.
 */
export function DiscardDialog(props: {
  target: DiscardTarget | undefined;
  onCancel: () => void;
  onDiscard: (target: DiscardTarget) => void;
}) {
  // The last ones asked about, still named while the dialog closes.
  const target = createMemo((last: DiscardTarget | undefined) => props.target ?? last);
  const title = () => {
    const current = target();
    if (current === "all") return "Discard all changes?";
    const path = current?.file.path ?? "";
    return `Discard changes to ${path.slice(path.lastIndexOf("/") + 1)}?`;
  };
  return (
    <AlertDialog
      open={props.target !== undefined}
      onOpenChange={(open) => !open && props.onCancel()}
      modal
      preventScroll
    >
      <DialogPortal>
        <AlertDialog.Content class={cn(DIALOG_BOX, "max-w-[440px] p-5")}>
          <div class="flex items-start gap-3">
            <span class="grid size-9 shrink-0 place-items-center rounded-[9px] bg-coral-soft text-coral">
              <Undo2 size={18} strokeWidth={1.9} />
            </span>
            <div class="min-w-0">
              <AlertDialog.Title class="m-0 text-[15px] font-[680] break-words">
                {title()}
              </AlertDialog.Title>
              <AlertDialog.Description class="m-0 mt-2 text-[12.5px] leading-[1.55] break-words text-text-soft">
                {describe(target())}
              </AlertDialog.Description>
            </div>
          </div>
          <div class="mt-5 flex justify-end gap-2">
            <Button variant="ghost" onClick={props.onCancel}>
              Cancel
            </Button>
            <Button variant="primary" onClick={() => props.target && props.onDiscard(props.target)}>
              Discard
            </Button>
          </div>
        </AlertDialog.Content>
      </DialogPortal>
    </AlertDialog>
  );
}

/** What discarding `target` does, as the dialog says it. */
function describe(target: DiscardTarget | undefined): string {
  if (target === undefined) return "";
  if (target === "all") {
    return "Every staged and unstaged change is lost, and untracked files are deleted. Ignored files are kept, and so are submodules' changes.";
  }
  const { side, file } = target;
  if (side === "unstaged") {
    // Added or renamed, the file was only marked to be added (`add -N`), and isn't staged.
    if (file.status === "untracked") return `${file.path} isn't tracked, so it's deleted.`;
    if (file.status === "added") return `${file.path} isn't staged yet, so it's deleted.`;
    if (file.status === "renamed" && file.origPath) {
      return `${file.path} is deleted, and ${file.origPath} put back as it's staged.`;
    }
    return `The unstaged changes to ${file.path} are lost. What's staged of it is kept.`;
  }
  // A copy is new too: the file it's a copy of is left as it is.
  if (file.status === "added" || file.status === "copied") {
    return `${file.path} is new, so it's deleted, with its staged and unstaged changes.`;
  }
  if (file.status === "renamed" && file.origPath) {
    return `${file.path} is deleted, and ${file.origPath} put back as the last commit has it: the staged and unstaged changes are lost.`;
  }
  return `${file.path} is put back as the last commit has it: its staged and unstaged changes are lost.`;
}
