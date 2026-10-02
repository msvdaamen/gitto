import { createORPCClient } from "@orpc/client";
import { RPCLink } from "@orpc/client/message-port";
import type { ContractRouterClient } from "@orpc/contract";

// Type-only, so the renderer bundle doesn't include the schemas.
import type { backendContract, contract, mainContract } from "./contract";
import { RPC_CONNECT_CHANNEL } from "./index";

export type RpcClient = ContractRouterClient<typeof contract>;

/**
 * Returns a typed client for the whole API. It's served by two processes (see the contracts), each
 * over a MessageChannel of its own: the preload forwards the other ends to the main process, since
 * MessagePorts can't cross the contextBridge, and that hands the backend's on. Calls to the backend
 * then go straight there, without passing through the main process.
 */
export function createRpcClient(): RpcClient {
  const main = new MessageChannel();
  const backend = new MessageChannel();
  // In this order: see `RPC_CONNECT_CHANNEL`.
  window.postMessage(RPC_CONNECT_CHANNEL, window.location.origin, [main.port2, backend.port2]);
  main.port1.start();
  backend.port1.start();

  const mainClient: ContractRouterClient<typeof mainContract> = createORPCClient(
    new RPCLink({ port: main.port1 }),
  );
  const backendClient: ContractRouterClient<typeof backendContract> = createORPCClient(
    new RPCLink({ port: backend.port1 }),
  );
  return {
    system: mainClient.system,
    repository: backendClient.repository,
    git: backendClient.git,
  };
}
