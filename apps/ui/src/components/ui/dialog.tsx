import { Dialog } from "@kobalte/core/dialog";
import type { JSX } from "solid-js";

/** Classes for a dialog's box, its `Content`; with a width and padding of its own added. */
export const DIALOG_BOX =
  "w-full animate-toast-in rounded-xl border border-border bg-panel text-text shadow-app motion-reduce:animate-none";

/**
 * A modal dialog's portal: its box, `children`, centred over the app, which is dimmed behind it.
 * For a Kobalte `Dialog` or `AlertDialog`, which share it.
 */
export function DialogPortal(props: { children: JSX.Element }) {
  return (
    <Dialog.Portal>
      <Dialog.Overlay class="fixed inset-0 z-50 bg-[rgba(10,6,14,.55)] backdrop-blur-[2px]" />
      <div class="fixed inset-0 z-50 grid place-items-center p-4">{props.children}</div>
    </Dialog.Portal>
  );
}
