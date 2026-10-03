import { cn } from "cn";
import type { LucideIcon } from "lucide-solid";
import LoaderCircle from "lucide-solid/icons/loader-circle";
import { Show, splitProps, type JSX } from "solid-js";
import { Dynamic } from "solid-js/web";

// Spelled out, so Tailwind finds the classes.
const HIDDEN_BELOW = { sm: "max-sm:hidden", lg: "max-lg:hidden" };

/**
 * One of the repository toolbar's actions: an icon over its label. Other attributes, and a ref, go
 * to the button, so it can open a popover (as a Kobalte trigger's `as`).
 */
export function ToolbarButton(
  props: Omit<JSX.ButtonHTMLAttributes<HTMLButtonElement>, "onClick" | "title" | "disabled"> & {
    icon: LucideIcon;
    label: string;
    /** Shown on hover; defaults to the label. */
    title?: string;
    accent?: boolean;
    /** Left out of the toolbar below this breakpoint, to make room. */
    hideBelow?: keyof typeof HIDDEN_BELOW;
    disabled?: boolean;
    /** Running: a spinner takes the icon's place. */
    busy?: boolean;
    class?: string;
    /** A count in the icon's corner, e.g. how many commits there are to pull; hidden when 0. */
    count?: number;
    onClick?: (event: MouseEvent) => void;
  },
) {
  const [local, others] = splitProps(props, [
    "icon",
    "label",
    "title",
    "accent",
    "hideBelow",
    "disabled",
    "busy",
    "class",
    "count",
    "onClick",
  ]);
  return (
    <button
      type="button"
      {...others}
      class={cn(
        "relative flex h-[38px] min-w-[43px] cursor-pointer flex-col items-center justify-center gap-0.5 rounded-md border-0 bg-transparent px-1.5 text-muted enabled:hover:bg-panel-hover enabled:hover:text-text focus-ring disabled:cursor-default max-md:min-w-9 max-md:[&>span]:hidden",
        local.accent && "text-blue",
        local.hideBelow && HIDDEN_BELOW[local.hideBelow],
        // Faded when it can't be used, but not while it's busy: that's still going on.
        local.disabled && !local.busy && "opacity-50",
        local.class,
      )}
      title={local.title ?? local.label}
      aria-label={local.label}
      aria-busy={local.busy}
      disabled={local.disabled}
      // Read on each click: Solid binds a handler once.
      onClick={(event) => local.onClick?.(event)}
    >
      <Dynamic
        component={local.busy ? LoaderCircle : local.icon}
        size={16}
        class={cn(local.busy && "animate-spin motion-reduce:animate-none")}
      />
      <span class="text-[10.5px]">{local.label}</span>
      <Show when={local.count}>
        <i class="absolute top-0.5 right-0.5 min-w-[15px] rounded-full bg-blue px-1 text-[9.5px] leading-[15px] font-[700] text-panel not-italic">
          {local.count}
        </i>
      </Show>
    </button>
  );
}
