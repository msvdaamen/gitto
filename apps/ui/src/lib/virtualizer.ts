import {
  elementScroll,
  observeElementOffset,
  observeElementRect,
  Virtualizer,
  type PartialKeys,
  type VirtualItem,
  type VirtualizerOptions,
} from "@tanstack/virtual-core";
import { createEffect, createSignal, createStore, onSettled, reconcile, untrack } from "solid-js";

export { defaultRangeExtractor, type Range } from "@tanstack/virtual-core";

type Options<TScroll extends Element, TItem extends Element> = PartialKeys<
  VirtualizerOptions<TScroll, TItem>,
  "observeElementRect" | "observeElementOffset" | "scrollToFn"
>;

/**
 * TanStack Virtual's virtualizer for a scroll element, as Solid state: `getVirtualItems()` and
 * `getTotalSize()` are tracked, and the options are read again whenever what they read changes
 * (pass the reactive ones as getters). The virtual items are kept by their index, so a row whose
 * item moves stays the same object, with its `start` updated.
 */
export function createVirtualizer<TScroll extends Element, TItem extends Element>(
  options: Options<TScroll, TItem>,
) {
  /** The options as they are now, read where it's tracked. */
  const resolve = (): VirtualizerOptions<TScroll, TItem> => ({
    observeElementRect,
    observeElementOffset,
    scrollToFn: elementScroll,
    ...options,
  });

  // Read again, tracked, by the effect below.
  const instance = new Virtualizer<TScroll, TItem>(untrack(resolve));
  const [items, setItems] = createStore<VirtualItem[]>(instance.getVirtualItems());
  const [totalSize, setTotalSize] = createSignal(instance.getTotalSize());

  const update = () => {
    setItems(reconcile(instance.getVirtualItems(), "index"));
    setTotalSize(instance.getTotalSize());
  };

  createEffect(
    () => {
      const resolved = resolve();
      // Read here, where it's tracked, rather than by the virtualizer as it updates: the scroll
      // element, as it comes and goes, and the size of an item.
      const scrollElement = resolved.getScrollElement();
      resolved.estimateSize(0);
      return { ...resolved, getScrollElement: () => scrollElement };
    },
    (resolved) => {
      instance.setOptions({
        ...resolved,
        onChange: (changed, sync) => {
          changed._willUpdate();
          update();
          resolved.onChange?.(changed, sync);
        },
      });
      instance._willUpdate();
      update();
    },
  );

  onSettled(() => {
    const cleanup = instance._didMount();
    instance._willUpdate();
    return cleanup;
  });

  return new Proxy(instance, {
    get(target, prop, receiver) {
      if (prop === "getVirtualItems") return () => items;
      if (prop === "getTotalSize") return () => totalSize();
      return Reflect.get(target, prop, receiver);
    },
  });
}
