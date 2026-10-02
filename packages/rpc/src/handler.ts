import { onError, type Context, type Router } from "@orpc/server";
import { RPCHandler } from "@orpc/server/message-port";

// GITTO_TRACE=1 also logs each call's duration, next to the git commands it ran.
const tracing = !!process.env.GITTO_TRACE;

/** Serves `router` over MessagePorts; call `upgrade(port, { context })` for each renderer connection. */
// oxlint-disable-next-line typescript/no-explicit-any -- any contract: the handler doesn't care which
export function createRpcHandler<T extends Context>(router: Router<any, T>) {
  return new RPCHandler(router, {
    interceptors: [
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
