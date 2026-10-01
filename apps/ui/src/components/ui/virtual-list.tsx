import { createVirtualizer } from "@tanstack/solid-virtual";
import { cn } from "cn";
import { createSignal, For, onCleanup, onMount, Show, type JSX } from "solid-js";

/** Rows rendered past each edge of the viewport, so scrolling doesn't reveal blank space. */
const OVERSCAN = 10;

/**
 * A scroll container that only renders the rows in view, so a list of thousands of rows costs as
 * much as a screenful. Every row is `rowHeight` tall, including any space below it.
 */
export function VirtualList<T>(props: {
  items: T[];
  rowHeight: number;
  /** Space above the first row and below the last one, in pixels. */
  padding?: number;
  /** Classes for the scroll container. */
  class?: string;
  children: (item: T) => JSX.Element;
}) {
  let container: HTMLDivElement | undefined;
  // The virtualizer takes the window to observe from the scroll container's document when it first
  // gets one, so only hand it over once it's on the page: on mount it can still be detached, being
  // rendered inside a `Suspense` boundary.
  const [scrollElement, setScrollElement] = createSignal<HTMLDivElement>();
  onMount(() => {
    let frame = 0;
    const attach = () => {
      if (container?.isConnected) setScrollElement(container);
      else frame = requestAnimationFrame(attach);
    };
    attach();
    onCleanup(() => cancelAnimationFrame(frame));
  });

  const virtualizer = createVirtualizer({
    get count() {
      return props.items.length;
    },
    getScrollElement: () => scrollElement() ?? null,
    estimateSize: () => props.rowHeight,
    get paddingStart() {
      return props.padding ?? 0;
    },
    get paddingEnd() {
      return props.padding ?? 0;
    },
    overscan: OVERSCAN,
  });

  return (
    <div ref={(el) => (container = el)} class={cn("min-h-0 overflow-y-auto", props.class)}>
      <div class="relative" style={{ height: `${virtualizer.getTotalSize()}px` }}>
        <For each={virtualizer.getVirtualItems()}>
          {(row) => (
            <div
              class="absolute inset-x-0 top-0"
              style={{ height: `${props.rowHeight}px`, transform: `translateY(${row.start}px)` }}
            >
              <Show when={props.items[row.index]} keyed>
                {(item) => props.children(item)}
              </Show>
            </div>
          )}
        </For>
      </div>
    </div>
  );
}
