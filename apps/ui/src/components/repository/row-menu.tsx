import { ContextMenu } from "@kobalte/core/context-menu";
import { useMenuContext } from "@kobalte/core/menu";
import { createEffect, createSignal, on, Show, type JSX } from "solid-js";

import { ContextMenuContent } from "@/components/ui/context-menu";

/**
 * The element marked with an item's key as `attribute` that `event` is on, if any. On a list that
 * has the focus, as when a key opens the menu, the first one in its active option: the history
 * keeps the focus on its list rather than on a row.
 */
function markedElement(event: Event, attribute: string): HTMLElement | null {
  if (!(event.target instanceof Element)) return null;
  const marked = event.target.closest<HTMLElement>(`[${attribute}]`);
  if (marked) return marked;
  const active = event.target.getAttribute("aria-activedescendant");
  const option = active ? document.getElementById(active) : null;
  return option?.querySelector<HTMLElement>(`[${attribute}]`) ?? null;
}

/**
 * Right-clicks `element`, below its start: a menu opened with a key on a list, again from the item
 * in its active option, to open by that rather than where the list is.
 */
function reopenAt(element: HTMLElement) {
  const box = element.getBoundingClientRect();
  element.dispatchEvent(
    new MouseEvent("contextmenu", {
      bubbles: true,
      cancelable: true,
      clientX: box.left,
      clientY: box.bottom,
    }),
  );
}

/**
 * A menu of what can be done with an item, opened by right-clicking an element in `children`
 * marked with the item's key as `data-<attribute>`, like a row in the sidebar or a label in the
 * history. Right-clicking anything else opens nothing. One menu for every element, rather than one
 * each: there can be thousands, made and dropped as the lists scroll. Elements rendered elsewhere
 * through a portal count too, as their events bubble through `children`.
 */
export function RowMenu<T>(props: {
  /** The repository the items are of: the menu closes once it changes. */
  repositoryId: string;
  /** The data attribute marking an element with its item's key, e.g. `branch` for `data-branch`. */
  attribute: string;
  /** The item by its key; `undefined` if it's gone. */
  item: (key: string) => T | undefined;
  /** Told which item's menu is open, by its key, if any, to show which one it's for. */
  onOpenFor: (key: string | undefined) => void;
  /** The menu's actions for the item it's open for. */
  actions: (item: () => T) => JSX.Element;
  children: JSX.Element;
}) {
  // The item it's open for, and what takes the focus back once it closes, kept as they were when
  // it opened.
  const [target, setTarget] = createSignal<{
    key: string;
    item: T;
    focus: HTMLElement | undefined;
  }>();

  /** Takes the item whose element `event` is on as the target; that element, if it has one. */
  const aim = (event: Event) => {
    const attribute = `data-${props.attribute}`;
    const element = markedElement(event, attribute);
    const key = element?.getAttribute(attribute) ?? undefined;
    const item = key === undefined ? undefined : props.item(key);
    // What has the focus: the row right-clicked, or the history's list, which keeps it rather than
    // its rows. Else the row, as at a long press, which focuses it only after this.
    const focused = document.activeElement;
    const focus =
      focused instanceof HTMLElement && focused !== document.body
        ? focused
        : (element?.closest("button") ?? undefined);
    setTarget(key !== undefined && item !== undefined ? { key, item, focus } : undefined);
    return item === undefined ? null : element;
  };

  return (
    <ContextMenu onOpenChange={(open) => props.onOpenFor(open ? target()?.key : undefined)}>
      <ContextMenu.Trigger
        as="div"
        // Laid out as if it weren't there, so what it holds is laid out as before.
        class="contents"
        // Read by Kobalte after the handlers below, which aim it: nothing opens off an item, nor at
        // a long press there.
        disabled={!target()}
        // Kobalte opens it where it was right-clicked, unless this is prevented.
        onContextMenu={(event) => {
          const element = aim(event);
          if (element?.contains(event.target as Node)) return;
          event.preventDefault();
          if (element) reopenAt(element);
        }}
        // Kobalte opens it at a long press of a finger or pen.
        onPointerDown={(event) => event.pointerType !== "mouse" && aim(event)}
      >
        {props.children}
      </ContextMenu.Trigger>
      <ContextMenuContent
        // Back to what had the focus, rather than to this, which can't take focus.
        onCloseAutoFocus={(event) => {
          const focus = target()?.focus;
          if (!focus?.isConnected) return;
          event.preventDefault();
          focus.focus({ preventScroll: true });
        }}
      >
        <CloseOnChange value={props.repositoryId} />
        <Show when={target()}>{(current) => props.actions(() => current().item)}</Show>
      </ContextMenuContent>
    </ContextMenu>
  );
}

/**
 * Closes the menu it's in once `value` changes, like the repository: another repository's item
 * isn't one of this one's.
 */
function CloseOnChange(props: { value: string }) {
  const menu = useMenuContext();
  createEffect(
    on(
      () => props.value,
      () => menu.close(),
      { defer: true },
    ),
  );
  return null;
}
