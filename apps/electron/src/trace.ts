/**
 * With GITTO_TRACE=1, logs whenever something keeps this process busy long enough to notice. For
 * the main process that stalls the UI: it routes the renderer's input, so while it's blocked, the
 * window can't respond. For the backend, calls from the UI wait that long.
 */
export function traceEventLoopStalls(name: "main" | "backend") {
  const INTERVAL_MS = 20;
  const STALL_MS = 50;
  let last = performance.now();
  setInterval(() => {
    const now = performance.now();
    const stall = now - last - INTERVAL_MS;
    if (stall >= STALL_MS) console.log(`[${name}] event loop blocked for ${stall.toFixed(0)}ms`);
    last = now;
  }, INTERVAL_MS).unref();
}
