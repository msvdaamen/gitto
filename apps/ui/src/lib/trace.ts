/** Frames that took this long are noticeable as jank: a click or scroll visibly lags. */
const LONG_FRAME_MS = 100;

// The parts of the Long Animation Frames API used here; TypeScript's DOM types don't have it yet.
interface LongAnimationFrame extends PerformanceEntry {
  scripts: { duration: number; invoker: string; sourceFunctionName: string }[];
}

/**
 * Logs frames the renderer took too long to draw, with the scripts that kept it busy. For the full
 * picture, record a profile in DevTools' Performance panel.
 */
export function traceLongFrames() {
  if (!PerformanceObserver.supportedEntryTypes.includes("long-animation-frame")) return;
  new PerformanceObserver((list) => {
    for (const frame of list.getEntries() as LongAnimationFrame[]) {
      if (frame.duration < LONG_FRAME_MS) continue;
      const scripts = frame.scripts
        .toSorted((a, b) => b.duration - a.duration)
        .slice(0, 3)
        .map(
          (script) =>
            `${script.sourceFunctionName || script.invoker} ${script.duration.toFixed(0)}ms`,
        );
      console.warn(`[ui] long frame: ${frame.duration.toFixed(0)}ms`, scripts);
    }
  }).observe({ type: "long-animation-frame", buffered: true });
}
