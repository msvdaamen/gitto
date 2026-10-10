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

/** A call to the router, as `RpcHandlerOptions.instrument` sees it. */
export interface RpcCall {
  /** The procedure's path, like `git.status`. */
  path: string;
  /** Aborted when the renderer cancels the call. */
  signal?: AbortSignal;
}

export interface RpcHandlerOptions {
  /** Wraps every call, to trace it or report its failure; `next` makes the call. */
  instrument?: <T>(call: RpcCall, next: () => Promise<T>) => Promise<T>;
}

/**
 * The code the renderer gets for an error a call threw: the one the procedure gave it, like
 * CONFLICT, or INTERNAL_SERVER_ERROR for one nothing expected.
 */
export function rpcErrorCode(error: unknown): string {
  return error instanceof ORPCError ? error.code : "INTERNAL_SERVER_ERROR";
}

/** Serves the router over MessagePorts; call `upgrade(port, { context })` for each renderer connection. */
export function createRpcHandler({ instrument }: RpcHandlerOptions = {}) {
  return new RPCHandler(router, {
    interceptors: [
      ({ next, request }) => {
        if (!instrument) return next();
        const path = request.url.pathname.replace(/^\//, "").replaceAll("/", ".");
        // Not `next` itself: what it's called with replaces the call's options.
        return instrument({ path, signal: request.signal }, () => next());
      },
      async ({ next, request }) => {
        if (!tracing) return next();
        const start = performance.now();
        try {
          return await next();
        } finally {
          const ms = (performance.now() - start).toFixed(0);
          console.log(`[rpc] ${ms.padStart(5)}ms ${request.url.pathname}`);
        }
      },
      onError((error, { request }) => {
        // Cancelled requests, like queries dropped when switching repositories, aren't failures.
        if (request.signal?.aborted) return;
        console.error("[rpc]", error);
      }),
    ],
  });
}
