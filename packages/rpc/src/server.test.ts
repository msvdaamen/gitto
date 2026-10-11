import type { UpdateState } from "@gitto/system/types";
import { createORPCClient } from "@orpc/client";
import { RPCLink } from "@orpc/client/message-port";
import { ORPCError } from "@orpc/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { RpcClient } from "./client";
import type { AppContext } from "./container";
import { createRpcHandler, type RpcCallEnd } from "./server";

const STATE: UpdateState = { version: "1.2.3", channel: "release", update: null };

let ports: MessagePort[] = [];

/** Serves `context` over a MessageChannel, as the main process does, with a client at the other end. */
function connect(context: Partial<AppContext>) {
  const calls: RpcCallEnd[] = [];
  const handler = createRpcHandler({ onCallEnd: (call) => calls.push(call) });
  const { port1, port2 } = new MessageChannel();
  ports.push(port1, port2);
  handler.upgrade(port2, { context: context as AppContext });
  port2.start();
  port1.start();
  const client: RpcClient = createORPCClient(new RPCLink({ port: port1 }));
  return { client, calls };
}

beforeEach(() => {
  vi.spyOn(console, "error").mockImplementation(() => {});
  vi.spyOn(console, "info").mockImplementation(() => {});
});

afterEach(() => {
  for (const port of ports) port.close();
  ports = [];
  vi.restoreAllMocks();
});

describe("createRpcHandler", () => {
  it("tells how a call ended, by its procedure's path", async () => {
    const { client, calls } = connect({ selectFolder: async () => "/picked" });

    await expect(client.system.selectFolder()).resolves.toBe("/picked");
    expect(calls).toEqual([
      { path: "system.selectFolder", duration: expect.any(Number), stream: false, outcome: "ok" },
    ]);
  });

  it("tells an expected failure by its code, and doesn't log it as an error", async () => {
    const error = new ORPCError("CONFLICT", { message: "index.lock exists" });
    const { client, calls } = connect({ selectFolder: () => Promise.reject(error) });

    await expect(client.system.selectFolder()).rejects.toMatchObject({ code: "CONFLICT" });
    expect(calls).toMatchObject([{ path: "system.selectFolder", outcome: "CONFLICT", error }]);
    expect(console.error).not.toHaveBeenCalled();
  });

  it("tells a failure nothing expected as INTERNAL_SERVER_ERROR, and logs it once", async () => {
    const error = new Error("boom");
    const { client, calls } = connect({ selectFolder: () => Promise.reject(error) });

    await expect(client.system.selectFolder()).rejects.toMatchObject({
      code: "INTERNAL_SERVER_ERROR",
    });
    expect(calls).toMatchObject([{ outcome: "INTERNAL_SERVER_ERROR", error }]);
    expect(console.error).toHaveBeenCalledOnce();
  });

  it("tells a stream that failed while it was read, once it has", async () => {
    const error = new Error("watcher died");
    const { client, calls } = connect({
      updates: {
        async *watch() {
          yield STATE;
          throw error;
        },
        install() {},
      },
    });

    const states: UpdateState[] = [];
    await expect(async () => {
      for await (const state of await client.system.update.watch()) states.push(state);
    }).rejects.toMatchObject({ code: "INTERNAL_SERVER_ERROR" });
    expect(states).toEqual([STATE]);
    expect(calls).toMatchObject([
      { path: "system.update.watch", stream: true, outcome: "INTERNAL_SERVER_ERROR", error },
    ]);
  });

  it("tells a stream the renderer stopped reading, and stops it", async () => {
    let stopped = false;
    const { client, calls } = connect({
      updates: {
        async *watch() {
          try {
            while (true) {
              yield STATE;
              // As a watcher waits for changes, rather than send as fast as it's read.
              // oxlint-disable-next-line no-await-in-loop -- one after the other, on purpose.
              await new Promise((resolve) => setTimeout(resolve, 10));
            }
          } finally {
            stopped = true;
          }
        },
        install() {},
      },
    });

    for await (const state of await client.system.update.watch()) {
      expect(state).toEqual(STATE);
      break;
    }
    await vi.waitFor(() => expect(calls).toHaveLength(1));
    expect(calls).toMatchObject([{ path: "system.update.watch", stream: true, outcome: "ok" }]);
    expect(stopped).toBe(true);
  });
});
