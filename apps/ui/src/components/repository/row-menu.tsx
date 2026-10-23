import { ContextMenu } from "@kobalte/core/context-menu";
import { useMenuContext } from "@kobalte/core/menu";
import {
  createContext,
  createEffect,
  createSignal,
  on,
  onCleanup,
  Show,
  useContext,
  type JSX,
} from "solid-js";

import { ContextMenuContent } from "@/components/ui/context-menu";

/** A `RowMenu`'s items, as the menus around and in it see them, to tell whose an element is. */
interface Layer {
  /** The data attribute marking its items' elements, e.g. `data-branch`. */
  attribute: string;
  /** Whether it has an item by the key `key`. */
  has: (key: string) => boolean;
}

/** What a `RowMenu` tells the ones in it: the menus around them, and how to join those. */
interface Nesting {
  /** The menus around, outermost first. */
  around: Layer[];
  /** Makes a menu in this one known to it, and to the ones around it, while it's there. */
  join: (layer: Layer) => void;
}

const Nesting = createContext<Nesting>({ around: [], join: () => {} });

/**
 * The element `event` is on that's an item of one of the menus `layers`, and which menu's, if any:
 * the innermost around what was right-clicked, the innermost menu's if it's an item of several. On
 * a list that has the focus, as when a key opens a menu, its active option's: the option itself,
 * else the first element in it that's an item. The history keeps the focus on its list rather than
 * on a row.
 */
function itemElement(
  event: Event,
  layers: Layer[],
): { element: HTMLElement; layer: Layer } | undefined {
  if (!(event.target instanceof Element)) return undefined;
  const selector = layers.map((layer) => `[${layer.attribute}]`).join(",");
  const owner = (element: Element) =>
    layers.findLast((layer) => {
      const key = element.getAttribute(layer.attribute);
      return key !== null && layer.has(key);
    });
  /** The innermost element that's an item, from `start` out. */
  const around = (start: Element) => {
    let element = start.closest<HTMLElement>(selector);
    while (element) {
      const layer = owner(element);
      if (layer) return { element, layer };
      element = element.parentElement?.closest<HTMLElement>(selector) ?? null;
    }
    return undefined;
  };
  const found = around(event.target);
  if (found) return found;
  const active = event.target.getAttribute("aria-activedescendant");
  const option = active ? document.getElementById(active) : null;
  if (!option) return undefined;
  const ofOption = around(option);
  if (ofOption) return ofOption;
  for (const element of option.querySelectorAll<HTMLElement>(selector)) {
    const layer = owner(element);
    if (layer) return { element, layer };
  }
  return undefined;
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
 * history. Right-clicking anything else opens nothing. A `RowMenu` can be in another, whose items'
 * elements hold its own: a commit's row in the history holds its branches' labels. An element then
 * opens the menu it's an item of, the innermost one's around what was right-clicked: a branch's
 * label opens the branch's menu, anywhere else on the row the commit's. A key opens the active
 * row's own menu, or else the first one of an item in it. One menu for every element, rather than
 * one each: there can be thousands, made and dropped as the lists scroll. Elements rendered
 * elsewhere through a portal count too, as their events bubble through `children`.
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

  const nesting = useContext(Nesting);
  const self: Layer = {
    get attribute() {
      return `data-${props.attribute}`;
    },
    has: (key) => props.item(key) !== undefined,
  };
  // The menus in this one, whose items' elements can hold its own, or be held by them.
  const inside: Layer[] = [];
  const join = (layer: Layer) => {
    inside.push(layer);
    nesting.join(layer);
    onCleanup(() => inside.splice(inside.indexOf(layer), 1));
  };
  nesting.join(self);

  /**
   * Takes the item whose element `event` is on as the target, unless that element is another
   * menu's; this one's element, if it has one, and whether any menu has an item there.
   */
  const aim = (event: Event) => {
    const found = itemElement(event, [...nesting.around, self, ...inside]);
    const element = found?.layer === self ? found.element : undefined;
    const key = element?.getAttribute(self.attribute) ?? undefined;
    const item = key === undefined ? undefined : props.item(key);
    // What has the focus: the row right-clicked, or the history's list, which keeps it rather than
    // its rows. Else the row, as at a long press, which focuses it only after this.
    const focused = document.activeElement;
    const focus =
      focused instanceof HTMLElement && focused !== document.body
        ? focused
        : (element?.closest("button") ?? undefined);
    setTarget(key !== undefined && item !== undefined ? { key, item, focus } : undefined);
    return { element: item === undefined ? undefined : element, taken: found !== undefined };
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
          const { element, taken } = aim(event);
          if (element?.contains(event.target as Node)) return;
          if (element) {
            event.preventDefault();
            reopenAt(element);
          }
          // Else it's another menu's, around this one or in it, to open; or none's, and the
          // outermost keeps the browser's own from opening.
          else if (!taken && nesting.around.length === 0) event.preventDefault();
        }}
        // Kobalte opens it at a long press of a finger or pen.
        onPointerDown={(event) => event.pointerType !== "mouse" && aim(event)}
      >
        <Nesting.Provider value={{ around: [...nesting.around, self], join }}>
          {props.children}
        </Nesting.Provider>
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
