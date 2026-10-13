import { createRpcClient } from "@gitto/rpc/client";

/** Typed client for the Electron main process. Only connects when running inside Electron. */
export const rpc = createRpcClient();
