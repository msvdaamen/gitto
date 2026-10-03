import { createRoot, createSignal } from "solid-js";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useSettled } from "./settled";

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

function mount<T>(initial: T) {
  const [value, setValue] = createSignal(initial);
  const [settled, dispose] = createRoot((disposeRoot) => [useSettled(value, 100), disposeRoot]);
  return { setValue, settled, dispose };
}

describe("useSettled", () => {
  it("follows a change right away after a quiet spell", async () => {
    const { setValue, settled, dispose } = mount("a");
    await vi.advanceTimersByTimeAsync(500);

    setValue("b");
    expect(settled()).toBe("b");
    dispose();
  });

  it("skips to the last of the changes that follow quickly", async () => {
    const { setValue, settled, dispose } = mount("a");
    await vi.advanceTimersByTimeAsync(500);

    setValue("b");
    await vi.advanceTimersByTimeAsync(30);
    setValue("c");
    await vi.advanceTimersByTimeAsync(30);
    setValue("d");
    expect(settled()).toBe("b");
    await vi.advanceTimersByTimeAsync(99);
    expect(settled()).toBe("b");
    await vi.advanceTimersByTimeAsync(1);
    expect(settled()).toBe("d");
    dispose();
  });

  it("doesn't hold back a change from or to nothing", async () => {
    const { setValue, settled, dispose } = mount<string | undefined>("a");
    await vi.advanceTimersByTimeAsync(500);

    setValue("b");
    setValue(undefined);
    expect(settled()).toBeUndefined();
    setValue("c");
    expect(settled()).toBe("c");
    dispose();
  });
});
