import type { RpcCallEnd } from "@gitto/rpc/server";
import * as Sentry from "@sentry/electron/main";
import { app } from "electron";

/**
 * Reports crashes, errors, warnings and errors logged, metrics and performance traces to Sentry,
 * from the main process and, through it, the renderer (see apps/ui/src/lib/sentry.ts). Only builds
 * made with a SENTRY_DSN have one (see .github/workflows/release.yml); others send nothing.
 *
 * Call it before the app is ready, and after `userData` is set: Sentry keeps what it hasn't sent
 * yet there.
 */
export function initSentry() {
  if (!SENTRY_DSN) return;
  Sentry.init({
    dsn: SENTRY_DSN,
    // The release is Gitto@<version>, as the source maps are uploaded for (see vite.sentry.ts).
    environment: environment(app.getVersion()),
    // Every trace, while there are few enough to keep them all.
    tracesSampleRate: 1,
    integrations: [
      // From starting Gitto to its first page shown, the renderer's spans included.
      Sentry.startupTracingIntegration(),
      Sentry.consoleLoggingIntegration({ levels: ["warn", "error"] }),
      // Not the UI's own files, which the app:// protocol fetches from disk: a trace each, every
      // time Gitto starts.
      Sentry.electronNetIntegration({ tracing: (_method, url) => !url.startsWith("file:") }),
    ],
    // An RPC call's failure is reported as an issue (see `reportRpcCall`), not again as a log.
    beforeSendLog: (log) => (String(log.message).startsWith("[rpc]") ? null : log),
    // What's in people's repositories stays with them: no IP address, and no local variables,
    // which may hold file contents, paths or commit messages.
    dataCollection: { userInfo: false, stackFrameVariables: false },
  });
}

function environment(version: string) {
  if (!app.isPackaged) return "development";
  return version.includes("-nightly") ? "nightly" : "production";
}

/**
 * Records how long each RPC call took, and reports the failures that weren't expected: the ones the
 * renderer gets as INTERNAL_SERVER_ERROR, rather than one it explains, like a conflict.
 */
export function reportRpcCall(call: RpcCallEnd) {
  if (!Sentry.isEnabled()) return;
  if (call.outcome === "INTERNAL_SERVER_ERROR") {
    Sentry.captureException(call.error, { tags: { "rpc.method": call.path } });
  }
  // A stream lasts as long as the renderer watches it, which says nothing about how fast it is.
  if (call.stream) return;
  // A metric, not a trace: calls come in bursts whenever a repository changes, too many to keep
  // each one's.
  Sentry.metrics.distribution("rpc.duration", call.duration, {
    unit: "millisecond",
    attributes: { "rpc.method": call.path, outcome: call.outcome },
  });
}
