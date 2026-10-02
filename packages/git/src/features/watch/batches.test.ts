import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { Batches } from "./batches";

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("batches", () => {
  it("hands over what was added once it's quiet, each value once", async () => {
    const batches = new Batches<string>();
    const next = batches.stream().next();
    batches.add("a");
    batches.add("b");
    batches.add("a");
    await vi.advanceTimersByTimeAsync(300);
    expect(await next).toEqual({ value: ["a", "b"], done: false });
    batches.close();
  });

  it("doesn't hold a batch back forever while changes keep coming", async () => {
    const batches = new Batches<number>();
    const next = batches.stream().next();
    let handed: number[] | undefined;
    void next.then((result) => (handed = result.value));

    // Never quiet for as long as the debounce takes.
    for (let i = 0; i < 30; i++) {
      batches.add(i);
      // oxlint-disable-next-line no-await-in-loop -- time has to pass between the changes
      await vi.advanceTimersByTimeAsync(100);
      if (handed) break;
    }
    expect(handed).toHaveLength(20);
    batches.close();
  });
});
