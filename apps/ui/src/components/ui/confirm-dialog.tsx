import { AlertDialog } from "@kobalte/core/alert-dialog";
import { cn } from "cn";
import type { LucideIcon } from "lucide-solid";
import { createMemo, Show, type JSX } from "solid-js";

import { Button } from "./button";
import { DIALOG_BOX, DialogPortal } from "./dialog";

/**
 * Asks before doing something to `item` that can't be undone, like deleting it; open while there's
 * one. Esc, or clicking outside, cancels.
 */
export function ConfirmDialog<T>(props: {
  item: T | undefined;
  icon: LucideIcon;
  /** What it asks of the item, e.g. "Delete stash?". */
  title: (item: T) => string;
  /** What's done to the item, and what's lost with it. */
  description: (item: T) => JSX.Element;
  /** The button that does it, e.g. "Delete". */
  confirmLabel: string;
  /** Whether it can't be done now, e.g. while something else runs that it would get in the way of. */
  confirmDisabled?: boolean;
  onCancel: () => void;
  onConfirm: (item: T) => void;
}) {
  // The last one asked about, still named while the dialog closes.
  const item = createMemo((last: T | undefined) => props.item ?? last);
  return (
    <AlertDialog
      open={props.item !== undefined}
      onOpenChange={(open) => !open && props.onCancel()}
      modal
      preventScroll
    >
      <DialogPortal>
        <AlertDialog.Content class={cn(DIALOG_BOX, "max-w-[440px] p-5")}>
          <div class="flex items-start gap-3">
            <span class="grid size-9 shrink-0 place-items-center rounded-[9px] bg-coral-soft text-coral">
              <props.icon size={18} strokeWidth={1.9} />
            </span>
            <Show when={item()}>
              {(shown) => (
                <div class="min-w-0">
                  <AlertDialog.Title class="m-0 text-[15px] font-[680]">
                    {props.title(shown())}
                  </AlertDialog.Title>
                  <AlertDialog.Description class="m-0 mt-2 text-[12.5px] leading-[1.55] break-words text-text-soft">
                    {props.description(shown())}
                  </AlertDialog.Description>
                </div>
              )}
            </Show>
          </div>
          <div class="mt-5 flex justify-end gap-2">
            <Button variant="ghost" onClick={props.onCancel}>
              Cancel
            </Button>
            <Button
              variant="primary"
              disabled={props.confirmDisabled}
              onClick={() => props.item !== undefined && props.onConfirm(props.item)}
            >
              {props.confirmLabel}
            </Button>
          </div>
        </AlertDialog.Content>
      </DialogPortal>
    </AlertDialog>
  );
}
