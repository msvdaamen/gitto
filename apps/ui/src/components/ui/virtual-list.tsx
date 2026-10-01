import { cn } from "cn";
import {
  createContext,
  createMemo,
  createSignal,
  For,
  onCleanup,
  onMount,
  useContext,
  type Accessor,
  type JSX,
} from "solid-js";

/** Rows rendered past each edge of the viewport, so scrolling doesn't reveal blank space. */
const OVERSCAN = 10;

/** The scroll container, re-set (not equal to itself) whenever its viewport may have moved. */
const ScrollerContext = createContext<Accessor<HTMLElement | undefined>>();

/**
 * Lets the `VirtualList`s in `children` render only the rows in view of the scroll container this
 * is placed in, its parent element.
 */
export function VirtualViewport(props: { children: JSX.Element }) {
  const [scroller, setScroller] = createSignal<HTMLElement | undefined>(undefined, {
    equals: false,
  });
  let content: HTMLDivElement | undefined;

  onMount(() => {
    const element = content?.parentElement;
    if (!content || !element) return;
    // At most once a frame: on scroll, and when the container or its content resizes, e.g. a
    // section above a list expands and pushes the list down.
    let frame = 0;
    const update = () => {
      frame ||= requestAnimationFrame(() => {
        frame = 0;
        setScroller(element);
      });
    };
    const observer = new ResizeObserver(update);
    observer.observe(element);
    observer.observe(content);
    element.addEventListener("scroll", update, { passive: true });
    onCleanup(() => {
      cancelAnimationFrame(frame);
      observer.disconnect();
      element.removeEventListener("scroll", update);
    });
    setScroller(element);
  });

  return (
    <div ref={(el) => (content = el)}>
      <ScrollerContext.Provider value={scroller}>{props.children}</ScrollerContext.Provider>
    </div>
  );
}

/**
 * A list that only renders the rows in view of its `VirtualViewport`, so a list of thousands of
 * rows costs as much as a screenful. Every row must be `rowHeight` tall, including the space
 * below it. Outside a `VirtualViewport`, it renders every row.
 */
export function VirtualList<T>(props: {
  items: T[];
  rowHeight: number;
  /** Classes for the element holding the rendered rows. */
  class?: string;
  children: (item: T) => JSX.Element;
}) {
  const scroller = useContext(ScrollerContext);
  let list: HTMLDivElement | undefined;
  // The list can mount after its viewport, e.g. once a `Suspense` around it resolves.
  const [mounted, setMounted] = createSignal(false);
  onMount(() => setMounted(true));

  const range = createMemo(
    () => {
      const count = props.items.length;
      const element = scroller?.();
      if (!scroller) return { start: 0, end: count };
      // Not measured yet: a screenful, until the viewport reports in after mounting.
      if (!element || !list || !mounted()) return { start: 0, end: Math.min(count, 2 * OVERSCAN) };
      // The top of the viewport, measured from the top of the list.
      const top = element.getBoundingClientRect().top - list.getBoundingClientRect().top;
      const start = clamp(Math.floor(top / props.rowHeight) - OVERSCAN, 0, count);
      const end = clamp(
        Math.ceil((top + element.clientHeight) / props.rowHeight) + OVERSCAN,
        start,
        count,
      );
      return { start, end };
    },
    undefined,
    { equals: (a, b) => a.start === b.start && a.end === b.end },
  );
  const visible = createMemo(() => props.items.slice(range().start, range().end));

  return (
    <div
      ref={(el) => (list = el)}
      class="relative shrink-0"
      style={{ height: `${props.items.length * props.rowHeight}px` }}
    >
      <div
        class={cn("absolute inset-x-0", props.class)}
        style={{ top: `${range().start * props.rowHeight}px` }}
      >
        <For each={visible()}>{(item) => props.children(item)}</For>
      </div>
    </div>
  );
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}
