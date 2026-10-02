// Set GITTO_TRACE=1 to log every git command and watcher event with its timing, e.g. to see what
// switching to another editor makes Gitto do.

export const tracing = !!process.env.GITTO_TRACE;

export function trace(message: string): void {
  if (tracing) console.log(`[git] ${message}`);
}
