import { cn } from "cn";

/** A thin vertical line between groups of items in a row. */
export function Divider(props: { class?: string }) {
  return <span class={cn("mx-1 h-3 w-px shrink-0 bg-border", props.class)} aria-hidden="true" />;
}
