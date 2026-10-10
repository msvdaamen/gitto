import { createORPCClient } from "@orpc/client";
import { RPCLink } from "@orpc/client/message-port";
import { ORPCError } from "@orpc/server";
import { describe, expect, it } from "vitest";

import type { RpcClient } from "./client";
import type { AppContext } from "./container";
import { createRpcHandler, rpcErrorCode, type RpcCall } from "./server";

/** Serves `context` over a MessageChannel, as the main process does, with a client at the other end. */
function connect(handler: ReturnType<typeof createRpcHandler>, context: Partial<AppContext>) {
  const { port1, port2 } = new MessageChannel();
  handler.upgrade(port2, { context: context as AppContext });
  port2.start();
  port1.start();
  const client: RpcClient = createORPCClient(new RPCLink({ port: port1 }));
  return { client, close: () => port1.close() };
}

describe("createRpcHandler", () => {
  it("wraps each call in `instrument`, with its path", async () => {
    const calls: string[] = [];
    const handler = createRpcHandler({
      // Like Sentry's startSpan, which calls back with the span: `next` must not take it as options.
      instrument: (call: RpcCall, next) => {
        calls.push(call.path);
        return (next as unknown as (span: unknown) => ReturnType<typeof next>)({ spanId: "1" });
      },
    });
    const { client, close } = connect(handler, { selectFolder: async () => "/picked" });

    await expect(client.system.selectFolder()).resolves.toBe("/picked");
    expect(calls).toEqual(["system.selectFolder"]);
    close();
  });
});

describe("rpcErrorCode", () => {
  it("is the code a procedure gave its error", () => {
    expect(rpcErrorCode(new ORPCError("CONFLICT"))).toBe("CONFLICT");
  });

  it("is INTERNAL_SERVER_ERROR for one nothing expected", () => {
    expect(rpcErrorCode(new Error("boom"))).toBe("INTERNAL_SERVER_ERROR");
  });
});
