// How the history keeps up with scrolling, on a repository with thousands of commits: a graph with
// merges and several lanes, and branches and tags among it.
import { afterAll, afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { commands, page, userEvent } from "vitest/browser";

import { renderApp } from "./app";
import { expectWithin, measureFrames, printFrames } from "./frames";
import { fakeRepository } from "./repository";
import { server } from "./rpc";

vi.mock("@/lib/rpc", () => import("./rpc"));

const COMMITS = 5000;

describe("history", () => {
  let unmount: () => void;
  beforeEach(async () => {
    server.repository = fakeRepository(COMMITS);
    unmount = await renderApp();
  });
  afterEach(() => unmount());
  afterAll(() => printFrames(`History, ${COMMITS} commits`));

  it("scrolls steadily", async () => {
    const scroller = page.getByRole("main");
    await userEvent.hover(scroller);
    // A wheel spun fast: about 3 rows a notch, for a few seconds.
    const stats = await measureFrames(() =>
      commands.wheel({ deltaY: 100, steps: 120, interval: 1000 / 60 }),
    );
    expect(scroller.element().scrollTop).toBeGreaterThan(100 * 100);
    expectWithin("steady scroll", stats, { p95: 5, worst: 25 });
  });
});
