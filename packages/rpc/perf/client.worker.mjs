// The renderer's side of the benchmark (see main-thread.ts): a client in its own thread, so
// whatever it does to decode responses doesn't count as main-thread time.
/* oxlint-disable unicorn/require-post-message-target-origin -- worker ports, not windows */
import { parentPort, workerData } from "node:worker_threads";

import { createORPCClient } from "@orpc/client";
import { RPCLink } from "@orpc/client/message-port";

const { port } = workerData;
port.start?.();
const rpc = createORPCClient(new RPCLink({ port }));

const watches = new Map();

async function run({ op, args }) {
  switch (op) {
    case "call": {
      const [path, input] = args;
      const fn = path.split(".").reduce((node, key) => node[key], rpc);
      const result = await fn(input);
      return { bytes: JSON.stringify(result ?? null).length, result: summarize(result) };
    }
    case "watch": {
      const [name, input] = args;
      const controller = new AbortController();
      const stream = await rpc.git.watch[name](input, { signal: controller.signal });
      const events = [];
      watches.set(name, { controller, events });
      void (async () => {
        try {
          for await (const event of stream) {
            events.push(event);
            parentPort.postMessage({ type: "event", name, event });
          }
        } catch {}
      })();
      return {};
    }
    case "unwatch": {
      watches.get(args[0])?.controller.abort();
      watches.delete(args[0]);
      return {};
    }
  }
}

/** Small enough to send back: the version (for `since`) and the sizes. */
function summarize(result) {
  if (Array.isArray(result)) return { length: result.length };
  if (result && typeof result === "object") {
    return {
      version: result.version,
      unchanged: result.unchanged,
      id: result.id,
      staged: result.changes?.staged.length,
      unstaged: result.changes?.unstaged.length,
    };
  }
  return result;
}

parentPort.on("message", async ({ id, ...request }) => {
  try {
    parentPort.postMessage({ type: "result", id, value: await run(request) });
  } catch (error) {
    parentPort.postMessage({ type: "result", id, error: String(error?.message ?? error) });
  }
});
