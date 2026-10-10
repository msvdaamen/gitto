import { rpcErrorCode, type RpcCall } from "@gitto/rpc/server";
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
    ],
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
 * Traces each RPC call, records how long it took, and reports the errors that weren't expected:
 * the ones the renderer gets as INTERNAL_SERVER_ERROR, rather than one it explains, like a conflict.
 */
export async function instrumentRpcCall<T>(call: RpcCall, next: () => Promise<T>): Promise<T> {
  if (!Sentry.isEnabled()) return next();
  const start = performance.now();
  let outcome = "ok";
  try {
    return await Sentry.startSpan(
      { name: call.path, op: "rpc.server", attributes: { "rpc.method": call.path } },
      next,
    );
  } catch (error) {
    // Cancelled requests, like queries dropped when switching repositories, aren't failures.
    outcome = call.signal?.aborted ? "cancelled" : rpcErrorCode(error);
    if (outcome === "INTERNAL_SERVER_ERROR") {
      Sentry.captureException(error, { tags: { "rpc.method": call.path } });
    }
    throw error;
  } finally {
    Sentry.metrics.distribution("rpc.duration", performance.now() - start, {
      unit: "millisecond",
      attributes: { "rpc.method": call.path, outcome },
    });
  }
}
