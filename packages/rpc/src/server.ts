import { gitRouter } from "@gitto/git/server";
import { repositoryRouter } from "@gitto/repository/server";
import { systemRouter } from "@gitto/system/server";
import { implement, onError } from "@orpc/server";
import { RPCHandler } from "@orpc/server/message-port";

import type { AppContext } from "./container";
import { contract } from "./contract";

export { createContainer } from "./container";

export const router = implement(contract).$context<AppContext>().router({
  system: systemRouter,
  repository: repositoryRouter,
  git: gitRouter,
});

/** Serves the router over MessagePorts; call `upgrade(port, { context })` for each renderer connection. */
export function createRpcHandler() {
  return new RPCHandler(router, {
    interceptors: [
      onError((error, { request }) => {
        // Cancelled requests, like queries dropped when switching repositories, aren't failures.
        if (request.signal?.aborted) return;
        console.error("[rpc]", error);
      }),
    ],
  });
}
