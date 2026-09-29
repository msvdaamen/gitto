import { systemRouter } from "@gitto/system/server";
import { implement, onError } from "@orpc/server";
import { RPCHandler } from "@orpc/server/message-port";

import { contract } from "./contract";

export const router = implement(contract).router({
  system: systemRouter,
});

/** Serves the router over MessagePorts; call `upgrade(port)` for each renderer connection. */
export function createRpcHandler() {
  return new RPCHandler(router, {
    interceptors: [onError((error) => console.error("[rpc]", error))],
  });
}
