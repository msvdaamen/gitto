import { Popover } from "@kobalte/core/popover";
import { cn } from "cn";
import X from "lucide-solid/icons/x";
import { Show, type JSX } from "solid-js";

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
  return (
    <Popover
      open={!!props.error}
      onOpenChange={(open) => !open && props.onDismiss()}
      placement={props.placement ?? "bottom-start"}
      gutter={6}
      anchorRef={props.anchor}
    >
      <Show when={!props.anchor}>
        <Popover.Anchor class={cn("flex", props.class)}>{props.children}</Popover.Anchor>
      </Show>
      <Popover.Portal>
        <Popover.Content
          // Not focused: the action can fail while the user is typing elsewhere.
          onOpenAutoFocus={(event) => event.preventDefault()}
          // Nor closed when focus moves elsewhere, e.g. back to the button from a popover it
          // opened: only by clicking outside, Escape, or dismissing it.
          onFocusOutside={(event) => event.preventDefault()}
          class="z-50 flex max-w-[360px] animate-toast-in items-start gap-2 rounded-lg border border-[color-mix(in_srgb,var(--coral)_30%,var(--border))] bg-panel-raised p-3 text-text shadow-app motion-reduce:animate-none"
        >
          <div class="min-w-0">
            <Popover.Title class="m-0 text-[12.5px] font-[680]">{props.title}</Popover.Title>
            <Popover.Description class="m-0 mt-1 text-[11.5px] break-words whitespace-pre-wrap text-coral">
              {props.error?.message}
            </Popover.Description>
          </div>
          <Popover.CloseButton
            class="grid size-5 shrink-0 cursor-pointer place-items-center rounded border-0 bg-transparent p-0 text-muted hover:bg-panel-hover hover:text-text focus-ring"
            aria-label="Dismiss"
          >
            <X size={13} />
          </Popover.CloseButton>
        </Popover.Content>
      </Popover.Portal>
    </Popover>
  );
}
