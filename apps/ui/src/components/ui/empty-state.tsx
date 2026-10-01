import { cn } from "cn";
import type { LucideIcon } from "lucide-solid";
import type { JSX } from "solid-js";
import { Dynamic } from "solid-js/web";

/** Centered placeholder for an empty, loading or failed panel. */
export function EmptyState(props: {
  icon: LucideIcon;
  title: string;
  children?: JSX.Element;
  tone?: "error";
  /** Whether it's waiting for something to load: the icon spins. */
  loading?: boolean;
  class?: string;
}) {
  return (
    <div
      role={props.loading ? "status" : undefined}
      class={cn(
        "flex h-[250px] flex-col items-center justify-center gap-1.5 px-6 text-center text-faint",
        props.class,
      )}
    >
      <Dynamic
        component={props.icon}
        size={22}
        class={props.loading ? "animate-spin motion-reduce:animate-none" : undefined}
      />
      <strong class="text-[11px] text-text-soft">{props.title}</strong>
      {props.children && (
        <span class={cn("text-[9px] whitespace-pre-wrap", props.tone === "error" && "text-coral")}>
          {props.children}
        </span>
      )}
    </div>
  );
}
