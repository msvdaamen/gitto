import type { RpcCallEnd } from "@gitto/rpc/server";
import { beforeEach, describe, expect, it, vi } from "vitest";

const Sentry = vi.hoisted(() => ({
  isEnabled: vi.fn(() => true),
  captureException: vi.fn(),
  metrics: { distribution: vi.fn() },
}));
vi.mock("@sentry/electron/main", () => Sentry);
vi.mock("electron", () => ({ app: {} }));

import { reportRpcCall } from "./sentry";

function call(end: Partial<RpcCallEnd>): RpcCallEnd {
  return { path: "git.status.get", duration: 12, stream: false, outcome: "ok", ...end };
}

beforeEach(() => vi.clearAllMocks());

describe("reportRpcCall", () => {
  it("records how long a call took, by procedure and outcome", () => {
    reportRpcCall(call({ outcome: "CONFLICT", error: new Error("index.lock exists") }));

    expect(Sentry.metrics.distribution).toHaveBeenCalledWith("rpc.duration", 12, {
      unit: "millisecond",
      attributes: { "rpc.method": "git.status.get", outcome: "CONFLICT" },
    });
  });

  it("reports only the failures nothing expected", () => {
    const error = new Error("boom");
    reportRpcCall(call({ outcome: "CONFLICT", error: new Error("index.lock exists") }));
    reportRpcCall(call({ outcome: "cancelled", error: new Error("aborted") }));
    reportRpcCall(call({ outcome: "INTERNAL_SERVER_ERROR", error }));

    expect(Sentry.captureException).toHaveBeenCalledExactlyOnceWith(error, {
      tags: { "rpc.method": "git.status.get" },
    });
  });

  it("reports a stream's failure, but not how long it was watched", () => {
    const error = new Error("watcher died");
    reportRpcCall(
      call({ stream: true, duration: 60_000, outcome: "INTERNAL_SERVER_ERROR", error }),
    );

    expect(Sentry.captureException).toHaveBeenCalledOnce();
    expect(Sentry.metrics.distribution).not.toHaveBeenCalled();
  });

  it("does nothing in a build that doesn't report to Sentry", () => {
    Sentry.isEnabled.mockReturnValueOnce(false);
    reportRpcCall(call({ outcome: "INTERNAL_SERVER_ERROR", error: new Error("boom") }));

    expect(Sentry.captureException).not.toHaveBeenCalled();
    expect(Sentry.metrics.distribution).not.toHaveBeenCalled();
  });
});
