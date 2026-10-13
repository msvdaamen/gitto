import { SEMANTIC_ATTRIBUTE_SENTRY_ORIGIN, startInactiveSpan } from "@sentry/electron/renderer";

type Span = ReturnType<typeof startInactiveSpan>;

/** How long nothing's traced before starting up counts as done. */
const IDLE_MS = 1000;
/** Ends it even if work's still going on, before the main process stops waiting for it. */
const FINAL_MS = 9000;

/** The renderer's part of starting up, while it's going on. */
let startup: Span | undefined;
/** What's traced as part of it and hasn't finished. */
let pending = 0;
/** When the last of it finished, in seconds, which is when starting up did. */
let lastEnd = 0;
let idleTimer: ReturnType<typeof setTimeout> | undefined;
let finalTimer: ReturnType<typeof setTimeout> | undefined;

/**
 * Times the renderer's part of starting up: from when the window started loading until what the
 * first screen asked for has come, and a second has gone by with nothing more asked for. The main
 * process's startup trace takes it in as the window's page load, which is why it's marked as one;
 * `browserTracingIntegration` would make one too, but traces everything after it as well.
 */
export function startStartup() {
  startup = startInactiveSpan({
    name: "startup",
    op: "pageload",
    startTime: performance.timeOrigin / 1000,
    forceTransaction: true,
    attributes: { [SEMANTIC_ATTRIBUTE_SENTRY_ORIGIN]: "auto.pageload.browser" },
  });
  lastEnd = now();
  waitForIdle();
  finalTimer = setTimeout(() => {
    // Ended while work's still going on: it took this long at least.
    if (pending) {
      startup?.setStatus({ code: 2, message: "deadline_exceeded" });
      lastEnd = now();
    }
    finish();
  }, FINAL_MS);
}

/**
 * Runs `work` as a part of starting up, while it's going on; otherwise just runs it. Work that
 * returns a promise is timed until it settles.
 */
export function traceStartup<T>(name: string, op: string, work: () => T): T {
  if (!startup) return work();
  const span = startInactiveSpan({ name, op, parentSpan: startup });
  pending++;
  clearTimeout(idleTimer);
  const settle = (ok: boolean) => {
    if (!ok) span.setStatus({ code: 2, message: "internal_error" });
    span.end();
    lastEnd = now();
    if (--pending === 0) waitForIdle();
  };
  try {
    const result = work();
    if (result instanceof Promise) {
      result.then(
        () => settle(true),
        () => settle(false),
      );
    } else {
      settle(true);
    }
    return result;
  } catch (error) {
    settle(false);
    throw error;
  }
}

function waitForIdle() {
  clearTimeout(idleTimer);
  idleTimer = setTimeout(finish, IDLE_MS);
}

function finish() {
  clearTimeout(idleTimer);
  clearTimeout(finalTimer);
  startup?.end(lastEnd);
  startup = undefined;
}

function now() {
  return (performance.timeOrigin + performance.now()) / 1000;
}
