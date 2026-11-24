/**
 * Classes for a dialog's box, its `DialogContent` or `AlertDialogContent`, centred over the app;
 * with a width (`max-w-…`) and padding of its own added.
 */
export const DIALOG_BOX =
  "fixed inset-0 z-50 m-auto h-fit max-h-[calc(100dvh-2rem)] w-[calc(100%-2rem)] overflow-y-auto animate-toast-in rounded-xl border border-border bg-panel text-text shadow-app outline-none motion-reduce:animate-none";

/** Classes for a modal dialog's `DialogBackdrop` or `AlertDialogBackdrop`: the app, dimmed. */
export const DIALOG_BACKDROP = "fixed inset-0 z-50 bg-[rgba(10,6,14,.55)] backdrop-blur-[2px]";
