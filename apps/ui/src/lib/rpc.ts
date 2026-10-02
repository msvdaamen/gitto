import { createRpcClient } from "@gitto/rpc/client";

/** Typed client for the app's API. Only connects when running inside Electron. */
export const rpc = createRpcClient();
