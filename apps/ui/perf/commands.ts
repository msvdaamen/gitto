// Browser commands for the renderer's perf suites. These run in Node, next to Playwright, and are
// called from the suites in the browser through `commands` from `vitest/browser`.
import { setTimeout as sleep } from "node:timers/promises";

import type { BrowserCommand } from "vitest/node";

interface Point {
  x: number;
  y: number;
}

interface TraceEvent {
  name: string;
  cat: string;
  ph: string;
  ts: number;
  dur?: number;
  tid: number;
}

/**
 * Starts recording what the renderer's main thread does, with Chrome's own tracing: it sees every
 * frame, where the Long Animation Frames API only reports the ones over 50ms.
 */
export const startFrameTrace: BrowserCommand<[]> = async ({ page }) => {
  await page
    .context()
    .browser()!
    .startTracing(page, { categories: ["toplevel", "devtools.timeline"] });
};

/**
 * Stops the recording, and returns how long the main thread was busy for each frame it drew, in
 * milliseconds: the input and scroll handlers, timers and rendering since the frame before. That's
 * the time a frame needs, which has to stay within the display's frame budget.
 */
export const stopFrameTrace: BrowserCommand<[], number[]> = async ({ page }) => {
  const buffer = await page.context().browser()!.stopTracing();
  const events: TraceEvent[] = JSON.parse(buffer.toString()).traceEvents;
  // A frame ends when its rendering does. Only the renderer's main thread renders frames.
  const frameEnds = events.filter((event) => event.name === "AnimationFrame" && event.ph === "e");
  const thread = frameEnds[0]?.tid;
  const ends = frameEnds.map((event) => event.ts).toSorted((a, b) => a - b);
  // Top-level tasks can nest (a microtask checkpoint runs inside a task), so they're merged into
  // the stretches the thread was busy for before being counted.
  const tasks = events
    .filter((event) => event.tid === thread && event.cat === "toplevel" && event.ph === "X")
    .map((event) => ({ start: event.ts, end: event.ts + (event.dur ?? 0) }))
    .toSorted((a, b) => a.start - b.start);
  const busy: { start: number; end: number }[] = [];
  for (const task of tasks) {
    const last = busy.at(-1);
    if (last && task.start <= last.end) last.end = Math.max(last.end, task.end);
    else busy.push({ ...task });
  }

  const frames = ends.map(() => 0);
  let frame = 0;
  for (const stretch of busy) {
    let start = stretch.start;
    // Work that runs past a frame's end goes towards the next frame, which it holds up.
    while (frame < ends.length && start < stretch.end) {
      const end = Math.min(stretch.end, ends[frame]!);
      if (end > start) frames[frame]! += end - start;
      if (stretch.end <= ends[frame]!) break;
      start = Math.max(start, ends[frame]!);
      frame++;
    }
  }
  return frames.map((microseconds) => microseconds / 1000);
};

/**
 * Turns the mouse wheel wherever the mouse is, as a person scrolling would: `steps` notches of
 * `deltaY` pixels, one every `interval` milliseconds, or as soon as the browser has taken the one
 * before. The input goes through the browser like a real wheel's, which scrolls it the way it does
 * those. Move the mouse first, before measuring: finding where an element is runs Playwright's
 * scripts in the page.
 */
export const wheel: BrowserCommand<
  [options: { deltaY: number; steps: number; interval: number }]
> = async ({ page }, { deltaY, steps, interval }) => {
  for (let step = 0; step < steps; step++) {
    // oxlint-disable-next-line no-await-in-loop -- one notch at a time, like a wheel turns
    await Promise.all([page.mouse.wheel(0, deltaY), sleep(interval)]);
  }
};

/**
 * Presses the mouse at `from` and drags it through `to`, one point every `interval` milliseconds,
 * like dragging a scrollbar's thumb. The points are in the page, not the iframe the suite runs in
 * (see `onPage`).
 */
export const drag: BrowserCommand<[from: Point, to: Point[], interval: number]> = async (
  { page },
  from,
  to,
  interval,
) => {
  await page.mouse.move(from.x, from.y);
  await page.mouse.down();
  for (const point of to) {
    // oxlint-disable-next-line no-await-in-loop -- one move at a time, like a hand
    await Promise.all([page.mouse.move(point.x, point.y), sleep(interval)]);
  }
  await page.mouse.up();
};
