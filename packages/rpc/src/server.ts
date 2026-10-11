import { gitRouter } from "@gitto/git/server";
import { repositoryRouter } from "@gitto/repository/server";
import { systemRouter } from "@gitto/system/server";
import { implement, onError, ORPCError } from "@orpc/server";
import { RPCHandler } from "@orpc/server/message-port";

import type { AppContext } from "./container";
import { contract } from "./contract";

export { createContainer } from "./container";

// GITTO_TRACE=1 also logs each call's duration, next to the git commands it ran.
const tracing = !!process.env.GITTO_TRACE;

export const router = implement(contract).$context<AppContext>().router({
  system: systemRouter,
  repository: repositoryRouter,
  git: gitRouter,
});

/** How a call to a procedure ended, as `RpcHandlerOptions.onCallEnd` hears it. */
export interface RpcCallEnd {
  /** The procedure's path, like `git.status.get`. */
  path: string;
  /** How long it took, in milliseconds: for a stream, like `git.watch.gitDir`, until it ended. */
  duration: number;
  stream: boolean;
  /**
   * `ok`, `cancelled` (the renderer stopped waiting, like for a query dropped when switching
   * repositories), or the code the renderer got for the error: the one the procedure gave it, like
   * CONFLICT, or INTERNAL_SERVER_ERROR for one nothing expected.
   */
  outcome: string;
  /** What it threw, unless it's `ok`. */
  error?: unknown;
}

export interface RpcHandlerOptions {
  /** Hears how each call ended, to record it or report its failure. */
  onCallEnd?: (call: RpcCallEnd) => void;
}

/** Serves the router over MessagePorts; call `upgrade(port, { context })` for each renderer connection. */
export function createRpcHandler({ onCallEnd }: RpcHandlerOptions = {}) {
  // Failures a procedure threw, so they're only logged once.
  const ended = new WeakSet<object>();

  function end(call: RpcCallEnd) {
    if (tracing) console.log(`[rpc] ${call.duration.toFixed(0).padStart(5)}ms ${call.path}`);
    if (call.outcome === "INTERNAL_SERVER_ERROR") {
      console.error(`[rpc] ${call.path}`, call.error);
    } else if (call.outcome !== "ok" && call.outcome !== "cancelled") {
      // Expected, and the renderer explains it: not an error of Gitto's.
      console.info(`[rpc] ${call.path}: ${call.outcome}`, (call.error as Error).message);
    }
    if (typeof call.error === "object" && call.error) ended.add(call.error);
    onCallEnd?.(call);
  }

  return new RPCHandler(router, {
    // Around the procedure only, after the request is decoded.
    clientInterceptors: [
      async ({ next, path, signal }) => {
        const start = performance.now();
        const ending = (stream: boolean, failed: boolean, error?: unknown): RpcCallEnd => ({
          path: path.join("."),
          duration: performance.now() - start,
          stream,
          outcome: !failed ? "ok" : signal?.aborted ? "cancelled" : errorCode(error),
          ...(failed && { error }),
        });
        let output: unknown;
        try {
          output = await next();
        } catch (error) {
          end(ending(false, true, error));
          throw error;
        }
        if (!isAsyncIterator(output)) {
          end(ending(false, false));
          return output;
        }
        // A stream's errors come while it's read, after `next` has returned it.
        return observeStream(output, (failed, error) => end(ending(true, failed, error)));
      },
    ],
    interceptors: [
      onError((error, { request }) => {
        // Failures before a procedure was called, like a request that couldn't be decoded.
        if (request.signal?.aborted || (typeof error === "object" && error && ended.has(error))) {
          return;
        }
        console.error("[rpc]", error);
      }),
    ],
  });
}

function errorCode(error: unknown): string {
  return error instanceof ORPCError ? error.code : "INTERNAL_SERVER_ERROR";
}

function isAsyncIterator(value: unknown): value is AsyncIterator<unknown> {
  return typeof value === "object" && value !== null && Symbol.asyncIterator in value;
}

/** Passes `stream` through, calling `done` once it ends: read to the end, stopped, or failed. */
async function* observeStream<T>(
  stream: AsyncIterator<T>,
  done: (failed: boolean, error?: unknown) => void,
): AsyncGenerator<T> {
  let failed = false;
  try {
    // Stopping this one, when the renderer stops reading, stops `stream` too.
    return yield* { [Symbol.asyncIterator]: () => stream };
  } catch (error) {
    failed = true;
    done(true, error);
    throw error;
  } finally {
    if (!failed) done(false);
  }
}
