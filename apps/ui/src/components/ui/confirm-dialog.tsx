import { AlertDialog } from "@kobalte/core/alert-dialog";
import { cn } from "cn";
import type { LucideIcon } from "lucide-solid";
import type { JSX } from "solid-js";
import { Dynamic } from "solid-js/web";

import { DIALOG_BOX, DialogPortal } from "./dialog";
import { toneClasses, type Tone } from "./tone";

/**
 * A modal that asks before going on with something, like deleting a stash: what it's about, with
 * an icon in its `tone`, and the buttons to answer with (`children`), Cancel first. Esc, or
 * clicking outside, cancels.
 */
export function ConfirmDialog(props: {
  open: boolean;
  icon: LucideIcon;
  /** Amber for something to think twice about, coral for something that's lost. */
  tone: Extract<Tone, "amber" | "coral">;
  title: string;
  description: JSX.Element;
  onCancel: () => void;
  children: JSX.Element;
}) {
  return (
    <AlertDialog
      open={props.open}
      onOpenChange={(open) => !open && props.onCancel()}
      modal
      preventScroll
    >
      <DialogPortal>
        <AlertDialog.Content class={cn(DIALOG_BOX, "max-w-[440px] p-5")}>
          <div class="flex items-start gap-3">
            <span
              class={cn(
                "grid size-9 shrink-0 place-items-center rounded-[9px]",
                toneClasses[props.tone],
              )}
            >
              <Dynamic component={props.icon} size={18} strokeWidth={1.9} />
            </span>
            <div class="min-w-0">
              <AlertDialog.Title class="m-0 text-[15px] font-[680]">
                {props.title}
              </AlertDialog.Title>
              <AlertDialog.Description class="m-0 mt-2 text-[12.5px] leading-[1.55] break-words text-text-soft">
                {props.description}
              </AlertDialog.Description>
            </div>
          </div>
          <div class="mt-5 flex justify-end gap-2">{props.children}</div>
        </AlertDialog.Content>
      </DialogPortal>
    </AlertDialog>
  );
}
