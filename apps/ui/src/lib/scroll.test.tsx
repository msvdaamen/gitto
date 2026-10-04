import { render, screen } from "@solidjs/testing-library";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/solid-router";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { SCROLL_TO_TOP, type ScrollId } from "./scroll";

/** Two repositories' pages in the history's scroll container, which stays on the page between them. */
function renderPages(scrollToTopSelectors?: string[]) {
  const root = createRootRoute({
    component: () => (
      <main data-scroll-restoration-id={"history" satisfies ScrollId}>
        <Outlet />
      </main>
    ),
  });
  const pages = ["first", "second"].map((name) =>
    createRoute({
      getParentRoute: () => root,
      path: `/${name}`,
      component: () => <p>Repository {name}</p>,
    }),
  );
  const router = createRouter({
    routeTree: root.addChildren(pages),
    history: createMemoryHistory({ initialEntries: ["/first"] }),
    scrollRestoration: true,
    scrollToTopSelectors,
  });
  render(() => <RouterProvider router={router} />);
  return router;
}

/** Scrolls the history down in the first repository, then shows the second: its scroll container. */
async function switchRepositories(scrollToTopSelectors?: string[]) {
  const router = renderPages(scrollToTopSelectors);
  await screen.findByText("Repository first");
  const scroller = screen.getByRole("main");
  scroller.scrollTop = 400;
  scroller.dispatchEvent(new Event("scroll"));

  router.history.push("/second");
  await screen.findByText("Repository second");
  return scroller;
}

describe("the history's scroll position", () => {
  const scrollTo = Element.prototype.scrollTo;

  beforeEach(() => {
    // jsdom doesn't scroll: scrolling to a position just sets it.
    Element.prototype.scrollTo = function (this: Element, options?: ScrollToOptions | number) {
      this.scrollTop = typeof options === "object" ? (options.top ?? 0) : 0;
    } as typeof Element.prototype.scrollTo;
    vi.spyOn(window, "scrollTo").mockImplementation(() => undefined);
    sessionStorage.clear();
  });

  afterEach(() => {
    Element.prototype.scrollTo = scrollTo;
    vi.restoreAllMocks();
  });

  it("starts at the top again when another repository is shown", async () => {
    const scroller = await switchRepositories(SCROLL_TO_TOP);
    await vi.waitFor(() => expect(scroller.scrollTop).toBe(0));
  });

  it("would stay where it was if the router weren't told to scroll it to the top", async () => {
    const scroller = await switchRepositories();
    // Long enough for the router to have scrolled it, if it did.
    await new Promise((resolve) => setTimeout(resolve, 100));
    expect(scroller.scrollTop).toBe(400);
  });
});
