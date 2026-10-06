import { cn } from "cn";
import type { LucideIcon } from "lucide-solid";
import LoaderCircle from "lucide-solid/icons/loader-circle";
import { Show, type JSX } from "solid-js";
import { Dynamic } from "solid-js/web";

import { useDelayed } from "@/hooks/delayed";

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
      <strong class="text-[13.5px] text-text-soft">{props.title}</strong>
      {props.children && (
        <span
          class={cn("text-[11.5px] whitespace-pre-wrap", props.tone === "error" && "text-coral")}
        >
          {props.children}
        </span>
      )}
    </div>
  );
}

/** How long something loads before `LoadingState` says so: a quick load doesn't flash it. */
const LOADING_SHOWN_AFTER_MS = 150;

/** An `EmptyState` saying `title` is loading, shown once that's taken a moment. */
export function LoadingState(props: { title: string; class?: string }) {
  const shown = useDelayed(() => true, LOADING_SHOWN_AFTER_MS);
  return (
    <Show when={shown()}>
      <EmptyState icon={LoaderCircle} loading title={props.title} class={props.class} />
    </Show>
  );
}
