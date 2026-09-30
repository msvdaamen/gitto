import { cn } from "cn";
import type { LucideIcon } from "lucide-solid";
import { Show, type JSX } from "solid-js";
import { Dynamic } from "solid-js/web";

import { Badge } from "./badge";

/** A panel section's title with its item count, and optional actions on the right. */
export function SectionHeader(props: {
  icon: LucideIcon;
  title: string;
  count?: number;
  tone?: "mint";
  class?: string;
  children?: JSX.Element;
}) {
  return (
    <div class={cn("flex shrink-0 items-center justify-between gap-2", props.class)}>
      <div class={cn("flex items-center gap-1.5", props.tone === "mint" && "text-mint")}>
        <Dynamic component={props.icon} size={15} />
        <strong class="text-[10px] text-text">{props.title}</strong>
        <Show when={props.count !== undefined}>
          <Badge tone={props.tone}>{props.count}</Badge>
        </Show>
      </div>
      {props.children}
    </div>
  );
}
