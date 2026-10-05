import { ContextMenu } from "@kobalte/core/context-menu";
import type { LucideIcon } from "lucide-solid";
import type { JSX } from "solid-js";

/** A context menu's list of actions, opened where it was right-clicked. */
export function ContextMenuContent(props: {
  /** Called as it closes, to move focus elsewhere than its trigger, preventing the event. */
  onCloseAutoFocus?: (event: Event) => void;
  children: JSX.Element;
}) {
  return (
    <ContextMenu.Portal>
      <ContextMenu.Content
        onCloseAutoFocus={(event) => props.onCloseAutoFocus?.(event)}
        class="z-50 min-w-[180px] animate-toast-in rounded-lg border border-border bg-panel-raised p-1 text-text shadow-app outline-none motion-reduce:animate-none"
      >
        {props.children}
      </ContextMenu.Content>
    </ContextMenu.Portal>
  );
}

/** One of a context menu's actions; the menu closes once it's chosen. */
export function ContextMenuItem(props: {
  icon: LucideIcon;
  label: string;
  disabled?: boolean;
  onSelect: () => void;
}) {
  return (
    <ContextMenu.Item
      class="flex h-7 cursor-pointer items-center gap-2 rounded-[5px] px-2 text-[12.5px] text-text-soft outline-none data-disabled:cursor-default data-disabled:opacity-50 data-highlighted:bg-panel-hover data-highlighted:text-text"
      disabled={props.disabled}
      // Read on each choice: Kobalte binds a handler once.
      onSelect={() => props.onSelect()}
    >
      <props.icon size={13} class="shrink-0" />
      <ContextMenu.ItemLabel class="truncate">{props.label}</ContextMenu.ItemLabel>
    </ContextMenu.Item>
  );
}
