// Measures the frames the renderer draws while a scenario runs, and reports them.
import { expect } from "vitest";
import { commands } from "vitest/browser";

declare module "vitest/browser" {
  interface BrowserCommands {
    startFrameTrace: () => Promise<void>;
    stopFrameTrace: () => Promise<number[]>;
    wheel: (options: { deltaY: number; steps: number; interval: number }) => Promise<void>;
    drag: (from: Point, to: Point[], interval: number) => Promise<void>;
  }
}

interface Point {
  x: number;
  y: number;
}

/**
 * Where the point `x`, `y` within `element` is on the page, for commands that move the mouse: the
 * suite runs in an iframe, which Vitest puts somewhere on its own page.
 */
export function onPage(element: Element, x: number, y: number): Point {
  const frame = window.frameElement?.getBoundingClientRect() ?? { left: 0, top: 0 };
  const box = element.getBoundingClientRect();
  return { x: frame.left + box.left + x, y: frame.top + box.top + y };
}

/** A 60Hz display's: a frame that takes longer misses its turn, and the UI visibly stutters. */
export const FRAME_BUDGET_MS = 1000 / 60;

export interface FrameStats {
  frames: number;
  median: number;
  p95: number;
  worst: number;
  /** Frames that took longer than `FRAME_BUDGET_MS`. */
  overBudget: number;
}

/**
 * The busy time of every frame drawn while `scenario` runs, and in the moment after, which draws
 * what it left pending. The waits are timers, not animation frames, so the only frames drawn are
 * the ones the app asks for.
 */
export async function measureFrames(scenario: () => Promise<void>): Promise<FrameStats> {
  await commands.startFrameTrace();
  await sleep(100);
  await scenario();
  await sleep(300);
  return summarize(await commands.stopFrameTrace());
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/** Resolves after `frames` frames. */
export async function quiet(frames = 10): Promise<void> {
  for (let i = 0; i < frames; i++) {
    // oxlint-disable-next-line no-await-in-loop -- one frame after another
    await new Promise(requestAnimationFrame);
  }
}

function summarize(frames: number[]): FrameStats {
  const sorted = frames.toSorted((a, b) => a - b);
  const at = (p: number) => sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))] ?? 0;
  return {
    frames: frames.length,
    median: at(0.5),
    p95: at(0.95),
    worst: sorted.at(-1) ?? 0,
    overBudget: frames.filter((frame) => frame > FRAME_BUDGET_MS).length,
  };
}

/**
 * What a scenario's frames may take, from what they took on the machine the suites were written on,
 * with headroom: `p95` catches every frame getting slower, `worst` a single slow one.
 */
export interface Budget {
  p95: number;
  worst: number;
}

const results: Record<string, string>[] = [];

const ms = (value: number) => (value < 10 ? value.toFixed(1) : value.toFixed(0));

/** Keeps `stats` for `printFrames` under `scenario`'s name, and fails if they're over `budget`. */
export function expectWithin(scenario: string, stats: FrameStats, budget: Budget) {
  results.push({
    scenario,
    frames: String(stats.frames),
    median: ms(stats.median),
    p95: ms(stats.p95),
    worst: ms(stats.worst),
    [`over ${FRAME_BUDGET_MS.toFixed(1)}`]: String(stats.overBudget),
    "budget p95/worst": `${ms(budget.p95)}/${ms(budget.worst)}`,
  });
  expect.soft(stats.p95, "95th percentile frame (ms)").toBeLessThanOrEqual(budget.p95);
  expect.soft(stats.worst, "worst frame (ms)").toBeLessThanOrEqual(budget.worst);
}

/** Prints the scenarios' frames as an aligned table, like the git package's perf suites do. */
export function printFrames(title: string) {
  const columns = Object.keys(results[0] ?? {});
  const widths = columns.map((column) =>
    Math.max(column.length, ...results.map((row) => row[column]?.length ?? 0)),
  );
  const line = (cells: string[]) =>
    cells
      .map((cell, i) => (i === 0 ? cell.padEnd(widths[i]!) : cell.padStart(widths[i]!)))
      .join("  ");
  console.log(
    `\n${title} (frame times in ms)\n${line(columns)}\n${results.map((row) => line(columns.map((c) => row[c] ?? ""))).join("\n")}`,
  );
}
