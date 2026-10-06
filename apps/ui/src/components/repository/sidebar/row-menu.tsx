import { ContextMenu } from "@kobalte/core/context-menu";
import { createSignal, Show, type JSX } from "solid-js";

import { ContextMenuContent } from "@/components/ui/context-menu";

/**
 * A menu of what can be done with an item, opened by right-clicking its row in `children`: one
 * marked with the item's key as `data-<attribute>`. Right-clicking anything else opens nothing. One
 * menu for every row, rather than one each: there can be thousands, made and dropped as the lists
 * scroll.
 */
export function RowMenu<T>(props: {
  /** The data attribute marking a row with its item's key, e.g. `branch` for `data-branch`. */
  attribute: string;
  /** The item by its key; `undefined` if it's gone. */
  item: (key: string) => T | undefined;
  /** Told which item's menu is open, by its key, if any, to show which one it's for. */
  onOpenFor: (key: string | undefined) => void;
  /** The menu's actions for the item it's open for. */
  actions: (item: () => T) => JSX.Element;
  children: JSX.Element;
}) {
  // The item it's open for, and its row, kept as they were when it opened.
  const [target, setTarget] = createSignal<{ key: string; item: T; row: HTMLElement }>();

  /** Takes the item whose row `event` is on as the target; whether there is one. */
  const aim = (event: Event) => {
    const row =
      event.target instanceof Element
        ? event.target.closest<HTMLElement>(`[data-${props.attribute}]`)
        : null;
    const key = row?.getAttribute(`data-${props.attribute}`) ?? undefined;
    const item = key === undefined ? undefined : props.item(key);
    setTarget(row && key !== undefined && item !== undefined ? { key, item, row } : undefined);
    return item !== undefined;
  };

  return (
    <ContextMenu onOpenChange={(open) => props.onOpenFor(open ? target()?.key : undefined)}>
      <ContextMenu.Trigger
        as="div"
        // Laid out as if it weren't there, so the sections it holds share the sidebar as before.
        class="contents"
        // Kobalte opens it unless this is prevented.
        onContextMenu={(event) => !aim(event) && event.preventDefault()}
        // Kobalte opens it at a long press of a finger or pen.
        onPointerDown={(event) => event.pointerType !== "mouse" && aim(event)}
      >
        {props.children}
      </ContextMenu.Trigger>
      <ContextMenuContent
        // Back to the row, rather than to this, which can't take focus.
        onCloseAutoFocus={(event) => {
          const row = target()?.row;
          if (!row?.isConnected) return;
          event.preventDefault();
          row.focus();
        }}
      >
        <Show when={target()}>{(current) => props.actions(() => current().item)}</Show>
      </ContextMenuContent>
    </ContextMenu>
  );
}
