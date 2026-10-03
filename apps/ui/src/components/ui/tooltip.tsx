import { Tooltip as KobalteTooltip, type TooltipRootProps } from "@kobalte/core/tooltip";
import { cn } from "cn";
import { splitProps, type JSX } from "solid-js";

/** How long the pointer rests on a tooltip's trigger before it opens. */
export const TOOLTIP_OPEN_DELAY = 150;

/**
 * A tooltip that opens quickly above its trigger. Put a `TooltipTrigger` (any element, through
 * `as`) and a `TooltipContent` inside it.
 */
export function Tooltip(props: TooltipRootProps & { children: JSX.Element }) {
  return (
    <KobalteTooltip
      openDelay={TOOLTIP_OPEN_DELAY}
      closeDelay={0}
      placement="top"
      gutter={6}
      {...props}
    >
      {props.children}
    </KobalteTooltip>
  );
}

export const TooltipTrigger = KobalteTooltip.Trigger;

/** The tooltip's bubble, rendered at the end of the page so scroll areas don't clip it. */
export function TooltipContent(props: { class?: string; children: JSX.Element }) {
  const [local, others] = splitProps(props, ["class", "children"]);
  return (
    <KobalteTooltip.Portal>
      <KobalteTooltip.Content
        class={cn(
          "z-50 max-w-[280px] animate-tooltip-in rounded-md border border-border bg-panel-hover px-2 py-1.5 text-[12px] text-text shadow-app motion-reduce:animate-none",
          local.class,
        )}
        {...others}
      >
        <KobalteTooltip.Arrow size={12} />
        {local.children}
      </KobalteTooltip.Content>
    </KobalteTooltip.Portal>
  );
}
