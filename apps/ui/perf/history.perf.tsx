// How the history keeps up with scrolling, on a repository with thousands of commits: a graph with
// merges and several lanes, and branches and tags among it.
import { afterAll, afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { commands, page, userEvent } from "vitest/browser";

import { renderApp } from "./app";
import { expectWithin, measureFrames, onPage, printFrames } from "./frames";
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
    expectWithin("steady scroll", stats, { p95: 6, worst: 20 });
  });

  it("jumps to where the scrollbar is dragged", async () => {
    const scroller = page.getByRole("main").element() as HTMLElement;
    // The thumb, at the top of the scrollbar (which has no arrow buttons) on the right.
    const x = scroller.clientWidth + (scroller.offsetWidth - scroller.clientWidth) / 2;
    const thumb = onPage(scroller, x, 1);
    // Back and forth across the history, far enough every time to replace every row in view.
    const stops = [0.5, 0.1, 0.9, 0.3, 0.7, 0.2, 0.8, 0.4, 0.6, 0.95, 0.05, 0.55, 0.15, 0.85];
    const to = stops.map((stop) => onPage(scroller, x, 1 + stop * scroller.clientHeight));
    const scrolled = new Set<number>();
    scroller.addEventListener("scroll", () => scrolled.add(scroller.scrollTop));
    const stats = await measureFrames(() => commands.drag(thumb, to, 200));
    expect(scrolled.size).toBeGreaterThanOrEqual(stops.length);
    expectWithin("scrollbar jumps", stats, { p95: 18, worst: 21 });
  });

  it("selects rows that are clicked", async () => {
    const stats = await measureFrames(() => commands.click(rowsInView().map(center), 300));
    expect(selected()).toHaveTextContent(rowsInView().at(-1)!.textContent!);
    expectWithin("click rows", stats, { p95: 20, worst: 20 });
  });

  it("moves the selection while Arrow Down is held", async () => {
    await commands.click([center(rowsInView()[0]!)], 0);
    // Three seconds at a usual key repeat rate, which scrolls the list along.
    const stats = await measureFrames(() => commands.press(Array(90).fill("ArrowDown"), 33));
    expect(selected()).toHaveAttribute("aria-posinset", "91");
    expectWithin("hold Arrow Down", stats, { p95: 8, worst: 14 });
  });

  it("moves the selection a page, and to either end, at a time", async () => {
    await commands.click([center(rowsInView()[0]!)], 0);
    const keys = [
      ...Array(8).fill("PageDown"),
      "End",
      ...Array(4).fill("PageUp"),
      "Home",
      ...Array(4).fill("PageDown"),
    ];
    const stats = await measureFrames(() => commands.press(keys, 250));
    const position = Number(selected().element().getAttribute("aria-posinset"));
    expect(position).toBeGreaterThan(4);
    expectWithin("Page Down, Up, Home, End", stats, { p95: 21, worst: 21 });
  });
});

/** The history's rows a click lands on: the ones in view, and not under its header. */
function rowsInView(): Element[] {
  return page
    .getByRole("option")
    .elements()
    .filter((row) => {
      const box = row.getBoundingClientRect();
      const top = document.elementFromPoint(box.left + box.width / 2, box.top + 1);
      const bottom = document.elementFromPoint(box.left + box.width / 2, box.bottom - 1);
      return row.contains(top) && row.contains(bottom);
    });
}

/** Where `row`'s middle is on the page. */
function center(row: Element) {
  const box = row.getBoundingClientRect();
  return onPage(row, box.width / 2, box.height / 2);
}

function selected() {
  return page.getByRole("option", { selected: true });
}
