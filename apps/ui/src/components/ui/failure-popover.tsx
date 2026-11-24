import X from "@lucide/solid/icons/x";
import type { JSX } from "@solidjs/web";
import {
  Popover,
  PopoverClose,
  PopoverContent,
  PopoverDescription,
  PopoverTitle,
} from "@ui-sdk/solid/popover";
import { cn } from "cn";
import { Show } from "solid-js";

/**
 * A button (or anything else that starts an action), with a popover under it that says why the
 * action failed until dismissed: e.g. the toolbar's Pull, or a tab's Remove. Or, given `anchor`,
 * the popover alone, placed by that element.
 */
export function FailurePopover(props: {
  /** The action, e.g. "Pull". */
  title: string;
  error: Error | null;
  onDismiss: () => void;
  /** Classes for the element around `children`, which the popover is placed under. */
  class?: string;
  /** Where the popover goes; under `children`, by default. */
  placement?: "bottom-start" | "right-start";
  /** The element the popover is placed by, rather than one around `children`. */
  anchor?: () => HTMLElement | undefined;
  children?: JSX.Element;
}) {
  let around: HTMLDivElement | undefined;

  return (
    <>
      <Show when={!props.anchor}>
        <div ref={(el) => (around = el)} class={cn("flex", props.class)}>
          {props.children}
        </div>
      </Show>
      <Popover
        open={!!props.error}
        onOpenChange={(open, details) => {
          // Not closed when focus moves elsewhere, e.g. back to the button from a popover it
          // opened: only by clicking outside, Escape, or dismissing it.
          if (!open && details.reason !== "focus-out" && details.reason !== "window-blur") {
            props.onDismiss();
          }
        }}
      >
        <PopoverContent
          anchor={(props.anchor ? props.anchor() : around) ?? null}
          side={props.placement === "right-start" ? "right" : "bottom"}
          align="start"
          sideOffset={6}
          // Not focused: the action can fail while the user is typing elsewhere.
          initialFocus="none"
          class="z-50 flex max-w-[360px] animate-toast-in items-start gap-2 rounded-lg border border-[color-mix(in_srgb,var(--coral)_30%,var(--border))] bg-panel-raised p-3 text-text shadow-app motion-reduce:animate-none"
        >
          <div class="min-w-0">
            <PopoverTitle class="m-0 text-[12.5px] font-[680]">{props.title}</PopoverTitle>
            <PopoverDescription class="m-0 mt-1 text-[11.5px] break-words whitespace-pre-wrap text-coral">
              {props.error?.message}
            </PopoverDescription>
          </div>
          <PopoverClose
            class="grid size-5 shrink-0 cursor-pointer place-items-center rounded border-0 bg-transparent p-0 text-muted hover:bg-panel-hover hover:text-text focus-ring"
            aria-label="Dismiss"
          >
            <X size={13} />
          </PopoverClose>
        </PopoverContent>
      </Popover>
    </>
  );
}
