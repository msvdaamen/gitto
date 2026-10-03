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
 * Stops the recording, and returns how long the main thread was busy in each frame it drew, in
 * milliseconds. Frames are as Chrome delimits them for the Long Animation Frames API: from the
 * first task after the frame before that led to this one (an input or scroll handler, a timer),
 * to the end of its rendering. Work in between that isn't followed by a frame, like garbage
 * collection while nothing happens, holds nothing up and isn't counted.
 */
export const stopFrameTrace: BrowserCommand<[], number[]> = async ({ page }) => {
  const buffer = await page.context().browser()!.stopTracing();
  const events: TraceEvent[] = JSON.parse(buffer.toString()).traceEvents;
  // Only the renderer's main thread draws frames, one at a time.
  const marks = events
    .filter((event) => event.name === "AnimationFrame" && (event.ph === "b" || event.ph === "e"))
    .toSorted((a, b) => a.ts - b.ts);
  const thread = marks[0]?.tid;
  const frames: { start: number; end: number }[] = [];
  let start: number | undefined;
  for (const mark of marks) {
    if (mark.tid !== thread) continue;
    if (mark.ph === "b") start = mark.ts;
    else if (start !== undefined) frames.push({ start, end: mark.ts });
  }
  // A frame can have gaps, e.g. between an input handler and the rendering it waits for, so only
  // the tasks in it count. Top-level tasks can nest (a microtask checkpoint runs inside a task).
  const tasks = events
    .filter((event) => event.tid === thread && event.cat === "toplevel" && event.ph === "X")
    .map((event) => ({ start: event.ts, end: event.ts + (event.dur ?? 0) }))
    .toSorted((a, b) => a.start - b.start);
  return frames.map((frame) => {
    let busy = frame.start;
    let total = 0;
    for (const task of tasks) {
      if (task.start >= frame.end) break;
      const from = Math.max(task.start, busy);
      const to = Math.min(task.end, frame.end);
      if (to > from) total += to - from;
      busy = Math.max(busy, to);
    }
    return total / 1000;
  });
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

/** Clicks each of `points` in turn, one every `interval` milliseconds. Points are as for `drag`. */
export const click: BrowserCommand<[points: Point[], interval: number]> = async (
  { page },
  points,
  interval,
) => {
  for (const point of points) {
    // oxlint-disable-next-line no-await-in-loop -- one click at a time, like a hand
    await Promise.all([page.mouse.click(point.x, point.y), sleep(interval)]);
  }
};

/**
 * Presses each of `keys` in turn, one every `interval` milliseconds. A key that's the same as the
 * one before is pressed again without being let go, the way a key held down repeats.
 */
export const press: BrowserCommand<[keys: string[], interval: number]> = async (
  { page },
  keys,
  interval,
) => {
  let held: string | undefined;
  for (const key of keys) {
    // oxlint-disable-next-line no-await-in-loop -- one key at a time, like a hand
    if (held && held !== key) await page.keyboard.up(held);
    held = key;
    // oxlint-disable-next-line no-await-in-loop
    await Promise.all([page.keyboard.down(key), sleep(interval)]);
  }
  if (held) await page.keyboard.up(held);
};
