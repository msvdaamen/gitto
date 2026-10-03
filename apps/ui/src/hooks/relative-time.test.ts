import { createEffect, createRoot } from "solid-js";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useRelativeTime } from "./relative-time";

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("useRelativeTime", () => {
  it("tells a time again as time passes", async () => {
    const made = Date.now() - 30_000;
    let shown = "";
    const dispose = createRoot((disposeRoot) => {
      const ago = useRelativeTime();
      createEffect(() => (shown = ago(made)));
      return disposeRoot;
    });
    expect(shown).toBe("Just now");

    await vi.advanceTimersByTimeAsync(60_000);
    expect(shown).toBe("1 minute ago");
    await vi.advanceTimersByTimeAsync(60 * 60_000);
    expect(shown).toBe("1 hour ago");
    dispose();
  });
});
