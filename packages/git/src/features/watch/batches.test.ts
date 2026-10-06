import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { Batches } from "./batches";

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

/** Collects the batches `batches` hands over, as they come. */
function collect<T>(batches: Batches<T>): T[][] {
  const handed: T[][] = [];
  void (async () => {
    for await (const batch of batches.stream()) handed.push(batch);
  })();
  return handed;
}

describe("Batches", () => {
  it("hands changes over together once it's quiet for a bit", async () => {
    const batches = new Batches<number>();
    const handed = collect(batches);

    batches.add(1);
    await vi.advanceTimersByTimeAsync(200);
    batches.add(2);
    await vi.advanceTimersByTimeAsync(299);
    expect(handed).toEqual([]);
    await vi.advanceTimersByTimeAsync(1);
    expect(handed).toEqual([[1, 2]]);

    batches.close();
  });

  it("takes several changes at once, as one batch", async () => {
    const batches = new Batches<number>();
    const handed = collect(batches);

    batches.addAll([1, 2]);
    batches.addAll([]);
    await vi.advanceTimersByTimeAsync(300);
    expect(handed).toEqual([[1, 2]]);

    batches.close();
  });

  it("doesn't hold changes back for as long as they keep coming", async () => {
    const batches = new Batches<number>();
    const handed = collect(batches);

    // A change every 100ms, for five seconds: never quiet for long enough.
    for (let i = 0; i < 50; i++) {
      batches.add(i);
      // oxlint-disable-next-line no-await-in-loop -- one change after the other, on purpose.
      await vi.advanceTimersByTimeAsync(100);
    }
    expect(handed.length).toBe(2);
    expect(handed.flat()).toEqual(Array.from({ length: 40 }, (_, i) => i));

    await vi.advanceTimersByTimeAsync(300);
    expect(handed.flat()).toEqual(Array.from({ length: 50 }, (_, i) => i));
    batches.close();
  });
});
