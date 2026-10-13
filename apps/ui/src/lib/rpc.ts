import { createRpcClient } from "@gitto/rpc/client";
import { traceStartup } from "@gitto/telemetry-ui";

/**
 * Typed client for the Electron main process. Only connects when running inside Electron. The calls
 * made while Gitto's starting are timed as part of it.
 */
export const rpc = createRpcClient({
  interceptors: [({ path, next }) => traceStartup(path.join("."), "rpc", next)],
});
