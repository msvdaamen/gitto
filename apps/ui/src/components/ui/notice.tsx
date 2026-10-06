import { cn } from "cn";
import type { LucideIcon } from "lucide-solid";
import FileWarning from "lucide-solid/icons/file-exclamation-point";
import { children, type JSX } from "solid-js";
import { Dynamic } from "solid-js/web";

/**
 * A line under a header about something that went wrong, like a file's edits that couldn't be
 * saved. A status by default: the user is told, but not interrupted; an alert for what they asked
 * for and didn't get, like lines that couldn't be staged.
 */
export function Notice(props: {
  message: string;
  role?: "status" | "alert";
  icon?: LucideIcon;
  /** Buttons for what can be done about it, after the message. */
  children?: JSX.Element;
}) {
  const actions = children(() => props.children);
  return (
    <p
      role={props.role ?? "status"}
      // Less padding around buttons, which are taller than the text: the same height either way.
      class={cn(
        "m-0 flex shrink-0 items-center gap-1.5 border-b border-border px-3 text-[11.5px] text-muted",
        actions() ? "py-1" : "py-1.5",
      )}
    >
      <Dynamic component={props.icon ?? FileWarning} size={13} class="shrink-0 text-amber" />
      <span class="mr-auto">{props.message}</span>
      {actions()}
    </p>
  );
}
