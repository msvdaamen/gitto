import type { LucideIcon } from "@lucide/solid";
import { Dynamic, type JSX } from "@solidjs/web";
import {
  AlertDialog,
  AlertDialogBackdrop,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogTitle,
} from "@ui-sdk/solid/alert-dialog";
import { cn } from "cn";

import { DIALOG_BACKDROP, DIALOG_BOX } from "./dialog";
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
      // An alert dialog stays open at a click outside, which cancels this one all the same.
      onDismissAttempt={(details) => details.reason === "outside-press" && props.onCancel()}
    >
      <AlertDialogBackdrop class={DIALOG_BACKDROP} />
      <AlertDialogContent class={cn(DIALOG_BOX, "max-w-[440px] p-5")}>
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
            <AlertDialogTitle class="m-0 text-[15px] font-[680]">{props.title}</AlertDialogTitle>
            <AlertDialogDescription class="m-0 mt-2 text-[12.5px] leading-[1.55] break-words text-text-soft">
              {props.description}
            </AlertDialogDescription>
          </div>
        </div>
        <div class="mt-5 flex justify-end gap-2">{props.children}</div>
      </AlertDialogContent>
    </AlertDialog>
  );
}
