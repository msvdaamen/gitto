import type { LucideIcon } from "@lucide/solid";
import type { JSX } from "@solidjs/web";
import { MenuContent, MenuItem } from "@ui-sdk/solid/menu";

/** A context menu's list of actions, opened where it was right-clicked. */
export function ContextMenuContent(props: {
  /** What takes the focus as it closes, rather than its trigger; that, if it's left out. */
  finalFocus?: () => HTMLElement | undefined;
  children: JSX.Element;
}) {
  return (
    <MenuContent
      finalFocus={props.finalFocus}
      class="z-50 min-w-[180px] animate-toast-in rounded-lg border border-border bg-panel-raised p-1 text-text shadow-app outline-none motion-reduce:animate-none"
    >
      {props.children}
    </MenuContent>
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
    <MenuItem
      class="flex h-7 cursor-pointer items-center gap-2 rounded-[5px] px-2 text-[12.5px] text-text-soft outline-none data-disabled:cursor-default data-disabled:opacity-50 data-highlighted:bg-panel-hover data-highlighted:text-text"
      disabled={props.disabled}
      // Read on each choice: a handler is bound once.
      onSelect={() => props.onSelect()}
    >
      <props.icon size={13} class="shrink-0" />
      <span class="truncate">{props.label}</span>
    </MenuItem>
  );
}
