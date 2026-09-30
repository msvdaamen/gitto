import { cn } from "cn";
import type { LucideIcon } from "lucide-solid";
import type { JSX } from "solid-js";
import { Dynamic } from "solid-js/web";

/** A small titled panel, with an optional action (like "View all") in its header. */
export function Card(props: {
  icon: LucideIcon;
  title: string;
  action?: JSX.Element;
  class?: string;
  children: JSX.Element;
}) {
  return (
    <section class={cn("rounded-[11px] border border-border bg-panel p-3", props.class)}>
      <div class="mb-1.25 flex h-7 items-center justify-between">
        <div class="flex items-center gap-1.75">
          <Dynamic component={props.icon} size={15} class="text-primary" />
          <h3 class="m-0 text-[11.5px]">{props.title}</h3>
        </div>
        {props.action}
      </div>
      {props.children}
    </section>
  );
}
