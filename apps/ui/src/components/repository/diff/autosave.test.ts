import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAutosave } from "./autosave";

/** An error as the main process sends it, by its code. */
const apiError = (code: string, message: string) => Object.assign(new Error(message), { code });

describe("autosave", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("saves once the edits stop, over the version the last save wrote", async () => {
    const save = vi.fn(async (text: string) => `v-${text}`);
    const autosave = createAutosave(save, 100);
    autosave.start("v0");

    autosave.change("a");
    autosave.change("ab");
    expect(autosave.state()).toEqual({ kind: "pending" });
    await vi.advanceTimersByTimeAsync(100);
    expect(save).toHaveBeenCalledExactlyOnceWith("ab", "v0", false);
    expect(autosave.state()).toEqual({ kind: "saved" });

    autosave.change("abc");
    await vi.advanceTimersByTimeAsync(100);
    expect(save).toHaveBeenLastCalledWith("abc", "v-ab", false);
  });

  it("saves what's left at once when flushed", async () => {
    const save = vi.fn(async () => "v1");
    const autosave = createAutosave(save, 100);
    autosave.start("v0");
    autosave.change("a");

    expect(await autosave.flush()).toBe(true);
    expect(save).toHaveBeenCalledOnce();
    expect(autosave.dirty()).toBe(false);
  });

  it("holds the edits once the file changed on disk, until it's overwritten", async () => {
    const save = vi.fn(async (_text: string, _version: string, overwrite: boolean) => {
      if (!overwrite)
        throw apiError("CONFLICT", "a.txt changed on disk since you started editing it.");
      return "v1";
    });
    const autosave = createAutosave(save, 100);
    autosave.start("v0");
    autosave.change("a");

    expect(await autosave.flush()).toBe(false);
    expect(autosave.state()).toMatchObject({ kind: "changed-on-disk" });
    // Further edits aren't saved over it either.
    autosave.change("ab");
    await vi.advanceTimersByTimeAsync(500);
    expect(save).toHaveBeenCalledOnce();

    expect(await autosave.overwrite()).toBe(true);
    expect(save).toHaveBeenLastCalledWith("ab", "v0", true);
  });

  it("tries a failed save again with the next edit", async () => {
    const save = vi
      .fn<(text: string) => Promise<string>>()
      .mockRejectedValueOnce(apiError("INTERNAL_SERVER_ERROR", "The disk is full."))
      .mockResolvedValue("v1");
    const autosave = createAutosave(save, 100);
    autosave.start("v0");
    autosave.change("a");
    await vi.advanceTimersByTimeAsync(100);
    expect(autosave.state()).toEqual({ kind: "failed", message: "The disk is full." });

    autosave.change("ab");
    await vi.advanceTimersByTimeAsync(100);
    expect(autosave.state()).toEqual({ kind: "saved" });
  });

  it("saves edits made while a save is under way after it", async () => {
    let finish!: (version: string) => void;
    const save = vi
      .fn<(text: string) => Promise<string>>()
      .mockImplementationOnce(() => new Promise((done) => (finish = done)))
      .mockResolvedValue("v2");
    const autosave = createAutosave(save, 100);
    autosave.start("v0");
    autosave.change("a");
    await vi.advanceTimersByTimeAsync(100);
    autosave.change("ab");
    finish("v1");
    await vi.advanceTimersByTimeAsync(100);
    expect(save).toHaveBeenLastCalledWith("ab", "v1", false);
    expect(autosave.state()).toEqual({ kind: "saved" });
  });

  it("sends what's left at once as the window closes, over the version a save under way started from", async () => {
    const save = vi
      .fn<(text: string, version: string) => Promise<string>>()
      .mockImplementationOnce(() => new Promise(() => {}))
      .mockResolvedValue("v2");
    const autosave = createAutosave(save, 100);
    autosave.start("v0");
    autosave.change("a");
    await vi.advanceTimersByTimeAsync(100);
    autosave.change("ab");

    autosave.flushNow();
    expect(save).toHaveBeenLastCalledWith("ab", "v0", false);
  });

  it("takes a file that's gone, or a repository, for a failed save, not one to overwrite", async () => {
    const save = vi.fn(async () => {
      throw apiError("NOT_FOUND", "Repository not found.");
    });
    const autosave = createAutosave(save, 100);
    autosave.start("v0");
    autosave.change("a");
    expect(await autosave.flush()).toBe(false);
    expect(autosave.state()).toEqual({ kind: "failed", message: "Repository not found." });
  });
});
