import { cn } from "cn";
import type { LucideIcon } from "lucide-solid";
import LoaderCircle from "lucide-solid/icons/loader-circle";
import { Show } from "solid-js";
import { Dynamic } from "solid-js/web";

// Spelled out, so Tailwind finds the classes.
const HIDDEN_BELOW = { sm: "max-sm:hidden", lg: "max-lg:hidden" };

/** One of the repository toolbar's actions: an icon over its label. */
export function ToolbarButton(props: {
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
  onClick?: () => void;
}) {
  return (
    <button
      type="button"
      class={cn(
        "relative flex h-[38px] min-w-[43px] cursor-pointer flex-col items-center justify-center gap-0.5 rounded-md border-0 bg-transparent px-1.5 text-muted enabled:hover:bg-panel-hover enabled:hover:text-text focus-ring disabled:cursor-default max-md:min-w-9 max-md:[&>span]:hidden",
        props.accent && "text-blue",
        props.hideBelow && HIDDEN_BELOW[props.hideBelow],
        // Faded when it can't be used, but not while it's busy: that's still going on.
        props.disabled && !props.busy && "opacity-50",
        props.class,
      )}
      title={props.title ?? props.label}
      aria-label={props.label}
      aria-busy={props.busy}
      disabled={props.disabled}
      // Read on each click: Solid binds a handler once.
      onClick={() => props.onClick?.()}
    >
      <Dynamic
        component={props.busy ? LoaderCircle : props.icon}
        size={16}
        class={cn(props.busy && "animate-spin motion-reduce:animate-none")}
      />
      <span class="text-[10.5px]">{props.label}</span>
      <Show when={props.count}>
        <i class="absolute top-0.5 right-0.5 min-w-[15px] rounded-full bg-blue px-1 text-[9.5px] leading-[15px] font-[700] text-panel not-italic">
          {props.count}
        </i>
      </Show>
    </button>
  );
}
