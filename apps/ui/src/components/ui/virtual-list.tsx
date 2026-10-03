import {
  createVirtualizer,
  defaultRangeExtractor,
  type Range,
  type VirtualItem,
} from "@tanstack/solid-virtual";
import { cn } from "cn";
import {
  createEffect,
  createMemo,
  createSignal,
  Index,
  onCleanup,
  onMount,
  Show,
  type JSX,
} from "solid-js";

/** Rows rendered past each edge of the viewport, so scrolling doesn't reveal blank space. */
const OVERSCAN = 10;

/**
 * A scroll container that only renders the rows in view, so a list of thousands of rows costs as
 * much as a screenful. Every row is `rowHeight` tall, including any space below it. Rows are
 * reused as the list scrolls (see `RecycledRows`), so what's rendered for one has to follow its item.
 */
export function VirtualList<T>(props: {
  items: T[];
  rowHeight: number;
  /** Space above the first row and below the last one, in pixels. */
  padding?: number;
  /** Classes for the scroll container. */
  class?: string;
  /** Renders a row; its item changes when the row is reused for another. */
  children: (item: () => T) => JSX.Element;
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
        <RecycledRows
          rows={virtualizer.getVirtualItems()}
          items={props.items}
          height={props.rowHeight}
        >
          {props.children}
        </RecycledRows>
      </div>
    </div>
  );
}

/**
 * The rows of a list inside a scroll container it shares with other content, e.g. a commit's files
 * below its message: only the rows in view are rendered, like in `VirtualList`, plus the one with
 * focus, so scrolling it out of view doesn't lose it. Every row is `rowHeight` tall, with `gap`
 * below it. Rows are reused as the list scrolls (see `RecycledRows`), and a refetch that replaces
 * the items updates the rows in place instead of re-creating them.
 */
export function VirtualRows<T>(props: {
  items: T[];
  rowHeight: number;
  /** Space below each row, in pixels. */
  gap?: number;
  /** The element that scrolls the rows, along with whatever is above and below them. */
  scrollElement: HTMLElement | undefined;
  /** Renders the item at `index`; both change when the row is reused for another. */
  children: (item: () => T, index: () => number) => JSX.Element;
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
      <RecycledRows
        rows={virtualizer.getVirtualItems()}
        items={props.items}
        height={props.rowHeight}
        offset={offset()}
      >
        {props.children}
      </RecycledRows>
    </div>
  );
}

/**
 * Renders `rows`, the ones of `items` a virtualizer says are in view, each in a slot that stays its
 * own for as long as it's rendered. A row that scrolls into view takes the slot of one that
 * scrolled out, so scrolling updates what the rows already there say, rather than building a row
 * and throwing one away for each that passes by.
 */
function RecycledRows<T>(props: {
  rows: VirtualItem[];
  items: T[];
  /** Height of a row, in pixels. */
  height: number;
  /** Where the rows' container starts in the scrolled content, which their positions include. */
  offset?: number;
  children: (item: () => T, index: () => number) => JSX.Element;
}) {
  // The slot each rendered row is in, by the row's index.
  let slotOf = new Map<number, number>();
  // The row in each slot; `undefined` for one that's free. A slot is never dropped: how many rows
  // fit in view goes up and down by one all the time while scrolling.
  const slots = createMemo<(VirtualItem | undefined)[]>((last) => {
    const next: (VirtualItem | undefined)[] = last.map(() => undefined);
    const nextSlotOf = new Map<number, number>();
    const entering: VirtualItem[] = [];
    for (const row of props.rows) {
      const slot = slotOf.get(row.index);
      if (slot === undefined) {
        entering.push(row);
      } else {
        next[slot] = row;
        nextSlotOf.set(row.index, slot);
      }
    }
    // Into the free slots, and new ones once those run out.
    let free = 0;
    for (const row of entering) {
      while (next[free] !== undefined) free++;
      next[free] = row;
      nextSlotOf.set(row.index, free);
    }
    slotOf = nextSlotOf;
    return next;
  }, []);

  return (
    <Index each={slots()}>
      {(slot) => {
        // The slot's row; the last one while it's free, when it's hidden, so what's rendered in it
        // is kept for the next row.
        const row = createMemo<VirtualItem | undefined>((last) => slot() ?? last);
        const index = () => row()?.index ?? 0;
        // The row's item; when the list shrinks past the row, the last one until the row is
        // hidden. Effects in the row can still run in between (e.g. ones a loading Suspense
        // boundary held back), and reading a <Show>'s stale value there would throw.
        const item = createMemo<T | undefined>((last) => props.items[index()] ?? last);
        return (
          <div
            data-index={index()}
            // Contained: rendering another item in a row doesn't lay out the rows around it.
            class="absolute inset-x-0 top-0 contain-layout contain-size contain-style"
            style={{
              height: `${props.height}px`,
              transform: `translateY(${(row()?.start ?? 0) - (props.offset ?? 0)}px)`,
              display: slot() ? undefined : "none",
            }}
          >
            <Show when={item() !== undefined}>{props.children(item as () => T, index)}</Show>
          </div>
        );
      }}
    </Index>
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
