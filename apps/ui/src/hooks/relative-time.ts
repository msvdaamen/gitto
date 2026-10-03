import { createSignal } from "solid-js";

import { relativeTime } from "@/lib/format";

/** How often relative times are brought up to date: they're told to the minute at most. */
const TICK_MS = 60_000;

// One clock for the whole app, started when a relative time is first shown and never stopped: a
// timer per row would be set up again for every row that scrolls into view.
const [tick, setTick] = createSignal(0);
let timer: ReturnType<typeof setInterval> | undefined;

/**
 * Tells a time (milliseconds since the epoch) relative to now, e.g. "3 days ago". What shows it is
 * updated as time passes, so a commit made "Just now" doesn't stay that until the history is
 * reloaded.
 */
export function useRelativeTime() {
  timer ??= setInterval(() => setTick((count) => count + 1), TICK_MS);
  return (timestamp: number) => {
    // Read to be run again on the next tick; the time itself is always the current one.
    tick();
    return relativeTime(timestamp);
  };
}
