// The backend process: the database, git and the file watchers, serving the renderer directly (see
// `backendContract`). Started by the main process as an Electron utility process.
import { createDb } from "@gitto/db";
import { createBackendContainer, createBackendRpcHandler } from "@gitto/rpc/backend";

import { BACKEND_ENV } from "./backend-env";
import { traceEventLoopStalls } from "./trace";

if (process.env.GITTO_TRACE) traceEventLoopStalls("backend");

const database = process.env[BACKEND_ENV.database];
const migrations = process.env[BACKEND_ENV.migrations];
if (!database || !migrations) throw new Error("The backend wasn't told where its database is.");

const db = createDb(database, migrations);
process.on("exit", () => db.$client.close());
// How the main process stops this when the app quits (see backend-process.ts). Node doesn't run
// the `exit` handlers on a signal by itself, and closing the database checkpoints its log.
process.on("SIGTERM", () => process.exit(0));

const handler = createBackendRpcHandler();
const context = createBackendContainer(db);

// Each renderer connection's MessagePort, handed on by the main process. Ports sent before this
// listens are queued, so none are missed while the database opens.
process.parentPort.on("message", (event) => {
  const [port] = event.ports;
  if (!port) return;
  handler.upgrade(port, { context });
  port.start();
});
