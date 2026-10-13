import { createORPCClient } from "@orpc/client";
import { RPCLink, type RPCLinkOptions } from "@orpc/client/message-port";
import type { ContractRouterClient } from "@orpc/contract";

// Type-only, so the renderer bundle doesn't include the schemas.
import type { contract } from "./contract";
import { RPC_CONNECT_CHANNEL } from "./index";

export type RpcClient = ContractRouterClient<typeof contract>;

/**
 * Opens a MessageChannel to the main process and returns a typed client over it. The preload
 * forwards the other end to main, since MessagePorts can't cross the contextBridge. `interceptors`
 * wrap each call.
 */
export function createRpcClient({
  interceptors,
}: Pick<RPCLinkOptions<object>, "interceptors"> = {}): RpcClient {
  const { port1: clientPort, port2: serverPort } = new MessageChannel();
  window.postMessage(RPC_CONNECT_CHANNEL, window.location.origin, [serverPort]);
  clientPort.start();
  return createORPCClient(new RPCLink({ port: clientPort, interceptors }));
}
