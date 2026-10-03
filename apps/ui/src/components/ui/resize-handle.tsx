import { cn } from "cn";
import { createSignal, onCleanup } from "solid-js";

import type { PanelBounds } from "@/hooks/panel-width";

const KEYBOARD_STEP = 16;

/** The x position of the handle's centre, which is the panel's edge. */
function center(handle: HTMLElement) {
  const rect = handle.getBoundingClientRect();
  return rect.left + rect.width / 2;
}

/** Keeps the resize cursor and stops text selection anywhere on the page while dragging. */
function setPageDragging(on: boolean) {
  document.documentElement.style.cursor = on ? "col-resize" : "";
  document.documentElement.style.userSelect = on ? "none" : "";
}

/**
 * A draggable line on the edge of a panel that resizes it. Sits in the panel's parent, centred on
 * the panel's edge: `start` for a panel on its left, `end` for one on its right. Double-click resets
 * the width; the arrow keys nudge it.
 */
export function ResizeHandle(props: {
  edge: "start" | "end";
  label: string;
  width: number;
  bounds: PanelBounds;
  /** The width to resize to; `save` is false while dragging, until the handle is let go of. */
  onResize: (width: number, save: boolean) => void;
  onReset: () => void;
  class?: string;
}) {
  const [dragging, setDragging] = createSignal(false);
  // How far the pointer grabbed the handle from its centre, so the edge doesn't jump to it.
  let grabOffset = 0;

  // The width for an edge at `x`. The panel can be narrower than its saved width when the window
  // is small, so this is measured rather than read from `props.width`.
  const widthAt = (handle: HTMLElement, x: number) => {
    const container = handle.parentElement!.getBoundingClientRect();
    return props.edge === "start" ? x - container.left : container.right - x;
  };
  const renderedWidth = (handle: HTMLElement) => widthAt(handle, center(handle)) || props.width;

  onCleanup(() => dragging() && setPageDragging(false));

  const stop = (event: PointerEvent & { currentTarget: HTMLElement }) => {
    if (!dragging()) return;
    event.currentTarget.releasePointerCapture(event.pointerId);
    setDragging(false);
    setPageDragging(false);
    // The width it was dragged to, saved now rather than on every frame of the drag.
    props.onResize(props.width, true);
  };

  return (
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label={props.label}
      aria-valuenow={props.width}
      aria-valuemin={props.bounds.min}
      aria-valuemax={props.bounds.max}
      tabIndex={0}
      class={cn(
        "group absolute top-0 bottom-0 z-20 w-2 cursor-col-resize touch-none outline-none",
        props.edge === "start" ? "-translate-x-1/2" : "translate-x-1/2",
        props.class,
      )}
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        event.preventDefault();
        grabOffset = center(event.currentTarget) - event.clientX;
        event.currentTarget.setPointerCapture(event.pointerId);
        setDragging(true);
        setPageDragging(true);
      }}
      onPointerMove={(event) => {
        if (dragging()) {
          props.onResize(widthAt(event.currentTarget, event.clientX + grabOffset), false);
        }
      }}
      onPointerUp={stop}
      onPointerCancel={stop}
      onDblClick={() => props.onReset()}
      onKeyDown={(event) => {
        const grow = props.edge === "start" ? "ArrowRight" : "ArrowLeft";
        const shrink = props.edge === "start" ? "ArrowLeft" : "ArrowRight";
        const current = () => renderedWidth(event.currentTarget);
        const width =
          event.key === grow
            ? current() + KEYBOARD_STEP
            : event.key === shrink
              ? current() - KEYBOARD_STEP
              : event.key === "Home"
                ? props.bounds.min
                : event.key === "End"
                  ? props.bounds.max
                  : undefined;
        if (width === undefined) return;
        event.preventDefault();
        props.onResize(width, true);
      }}
    >
      <span
        class={cn(
          "pointer-events-none absolute inset-y-0 left-1/2 w-0.5 -translate-x-1/2 bg-primary transition-opacity duration-150 motion-reduce:transition-none",
          // Shown after a short hover, so moving the pointer across the edge doesn't flash it.
          dragging()
            ? "opacity-100"
            : "opacity-0 group-hover:opacity-100 group-hover:delay-200 group-focus-visible:opacity-100",
        )}
      />
    </div>
  );
}
