import { describe, expect, it, vi } from "vitest";

import { createRepo, repos } from "../../test/fixtures";
import { Turns, watchGitDir } from "./commands";

/** A watcher whose unsubscribes take a while, like removing a big repository's watches does. */
const watcher = vi.hoisted(() => ({
  subscribed: [] as string[],
  unsubscribing: 0,
  /** The folders subscribed to while an unsubscribe was still running. */
  overlapped: [] as string[],
}));

vi.mock("@parcel/watcher", () => ({
  subscribe: async (dir: string) => {
    if (watcher.unsubscribing > 0) watcher.overlapped.push(dir);
    watcher.subscribed.push(dir);
    return {
      unsubscribe: async () => {
        watcher.unsubscribing++;
        await new Promise((resolve) => setTimeout(resolve, 100));
        watcher.unsubscribing--;
      },
    };
  },
}));

describe("the watcher's subscriptions", () => {
  it("don't start while another is being removed", async () => {
    createRepo("turn-left");
    createRepo("turn-shown");
    const [left, shown] = await Promise.all([repos.open("turn-left"), repos.open("turn-shown")]);

    const leaving = new AbortController();
    const stopped = watchGitDir(left, leaving.signal).next();
    await vi.waitFor(() => expect(watcher.subscribed).toHaveLength(1));

    // As when switching repositories: the old one's watch stops as the new one's starts. A
    // subscribe during an unsubscribe can be left without events for good (see `watch`).
    const controller = new AbortController();
    leaving.abort();
    const started = watchGitDir(shown, controller.signal).next();
    await vi.waitFor(() => expect(watcher.subscribed).toHaveLength(2));
    expect(watcher.overlapped).toEqual([]);

    controller.abort();
    await Promise.all([stopped, started]);
  });

  it("start alongside each other, and ahead of an unsubscribe that's waiting for one", async () => {
    const turns = new Turns();
    const order: string[] = [];
    /** A task that says when it starts, and ends when `end` is called. */
    function task(name: string) {
      const { promise, resolve } = Promise.withResolvers<void>();
      return { end: resolve, run: () => (order.push(name), promise) };
    }
    const [walking, leaving, shown] = [task("walking"), task("leaving"), task("shown")];

    // The repository that's left is still starting to watch its folders when it's unwatched.
    const first = turns.take("subscribe", walking.run);
    const unsubscribed = turns.take("unsubscribe", leaving.run);
    // The one that's shown doesn't wait for either.
    const second = turns.take("subscribe", shown.run);
    expect(order).toEqual(["walking", "shown"]);

    shown.end();
    await second;
    expect(order).toEqual(["walking", "shown"]);
    walking.end();
    await first;
    await vi.waitFor(() => expect(order).toEqual(["walking", "shown", "leaving"]));
    leaving.end();
    await unsubscribed;
  });

  it("carry on after one fails", async () => {
    const turns = new Turns();
    await expect(turns.take("subscribe", () => Promise.reject(new Error("no")))).rejects.toThrow();
    await expect(
      turns.take("unsubscribe", () => {
        throw new Error("no");
      }),
    ).rejects.toThrow();
    expect(await turns.take("subscribe", async () => "watching")).toBe("watching");
  });
});
