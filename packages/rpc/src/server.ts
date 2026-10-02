import { gitRouter } from "@gitto/git/server";
import { repositoryRouter } from "@gitto/repository/server";
import { systemRouter } from "@gitto/system/server";
import { implement, onError } from "@orpc/server";
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

/** Serves the router over MessagePorts; call `upgrade(port, { context })` for each renderer connection. */
export function createRpcHandler() {
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
