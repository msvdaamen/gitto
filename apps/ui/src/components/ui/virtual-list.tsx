import { createVirtualizer, defaultRangeExtractor, type Range } from "@tanstack/solid-virtual";
import { cn } from "cn";
import {
  createEffect,
  createMemo,
  createSignal,
  For,
  onCleanup,
  onMount,
  Show,
  type JSX,
} from "solid-js";

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
  const connected = useConnected(() => container);
  const scrollElement = () => (connected() ? container : undefined);

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

/**
 * The rows of a list inside a scroll container it shares with other content, e.g. a commit's files
 * below its message: only the rows in view are rendered, like in `VirtualList`, plus the one with
 * focus, so scrolling it out of view doesn't lose it. Every row is `rowHeight` tall, with `gap`
 * below it. Rows are rendered by position, so a refetch that replaces the items updates the rows in
 * place instead of re-creating them.
 */
export function VirtualRows<T>(props: {
  items: T[];
  rowHeight: number;
  /** Space below each row, in pixels. */
  gap?: number;
  /** The element that scrolls the rows, along with whatever is above and below them. */
  scrollElement: HTMLElement | undefined;
  /** Renders the item at `index`; the item can change, as rows are rendered by position. */
  children: (item: () => T, index: number) => JSX.Element;
}) {
  let list: HTMLDivElement | undefined;
  const connected = useConnected(() => list);
  const scrollElement = () => (connected() ? props.scrollElement : undefined);

  // Where the list starts in the scroll container's content, which moves when the content above it
  // changes size. That content is somewhere inside the list's ancestors, so they're the ones watched.
  const [offset, setOffset] = createSignal(0);
  createEffect(() => {
    const scroller = scrollElement();
    if (!scroller || !list) return;
    const measure = () =>
      setOffset(
        list!.getBoundingClientRect().top -
          scroller.getBoundingClientRect().top -
          scroller.clientTop +
          scroller.scrollTop,
      );
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    for (let el = list.parentElement; el && el !== scroller; el = el.parentElement) {
      observer.observe(el);
    }
    onCleanup(() => observer.disconnect());
  });

  // The row with focus, if any, by its index.
  const [focused, setFocused] = createSignal<number>();
  const onFocusIn = (event: FocusEvent) => {
    const row = (event.target as Element).closest<HTMLElement>("[data-index]");
    setFocused(row ? Number(row.dataset.index) : undefined);
  };
  const onFocusOut = (event: FocusEvent) => {
    if (!list?.contains(event.relatedTarget as Node | null)) setFocused(undefined);
  };

  const virtualizer = createVirtualizer({
    get count() {
      return props.items.length;
    },
    getScrollElement: () => scrollElement() ?? null,
    estimateSize: () => props.rowHeight + (props.gap ?? 0),
    get scrollMargin() {
      return offset();
    },
    overscan: OVERSCAN,
    get rangeExtractor() {
      const index = focused();
      return (range: Range) => {
        const indexes = defaultRangeExtractor(range);
        if (index === undefined || index >= range.count || indexes.includes(index)) return indexes;
        return [...indexes, index].toSorted((a, b) => a - b);
      };
    },
  });

  return (
    <div
      ref={(el) => (list = el)}
      class="relative"
      style={{ height: `${virtualizer.getTotalSize()}px` }}
      onFocusIn={onFocusIn}
      onFocusOut={onFocusOut}
    >
      <For each={virtualizer.getVirtualItems()}>
        {(row) => {
          // The row's item; when the list shrinks past the row, the last one until the row is
          // removed. Effects in the row can still run in between (e.g. ones a loading Suspense
          // boundary held back), and reading a <Show>'s stale value there would throw.
          const item = createMemo<T | undefined>((last) => props.items[row.index] ?? last);
          return (
            <div
              data-index={row.index}
              class="absolute inset-x-0 top-0"
              style={{
                height: `${props.rowHeight}px`,
                transform: `translateY(${row.start - offset()}px)`,
              }}
            >
              <Show when={item() !== undefined}>{props.children(item as () => T, row.index)}</Show>
            </div>
          );
        }}
      </For>
    </div>
  );
}

/**
 * Whether `element` is on the page. The virtualizer takes the window to observe from the scroll
 * container's document when it first gets one, so it's only handed over once it's on the page: on
 * mount it can still be detached, being rendered inside a `Suspense` boundary.
 */
function useConnected(element: () => HTMLElement | undefined) {
  const [connected, setConnected] = createSignal(false);
  onMount(() => {
    let frame = 0;
    const check = () => {
      if (element()?.isConnected) setConnected(true);
      else frame = requestAnimationFrame(check);
    };
    check();
    onCleanup(() => cancelAnimationFrame(frame));
  });
  return connected;
}
