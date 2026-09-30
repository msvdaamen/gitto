import { cn } from "cn";
import type { JSX } from "solid-js";

/** A keyboard shortcut, like `⌘ K`. */
export function Kbd(props: { children: JSX.Element; class?: string }) {
  return (
    <kbd
      class={cn(
        "rounded-sm border border-border bg-panel-raised px-1.25 py-0.5 font-mono text-[10px] text-faint",
        props.class,
      )}
    >
      {props.children}
    </kbd>
  );
}
