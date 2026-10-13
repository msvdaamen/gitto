import { fireEvent, render, screen } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/solid-router";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { queryClient } from "@/lib/query-client";

import { Tabs } from "./tabs";

const rpc = vi.hoisted(() => ({
  repository: { list: vi.fn(), add: vi.fn(), remove: vi.fn(), reorder: vi.fn() },
  system: { selectFolder: vi.fn() },
}));

vi.mock("@/lib/rpc", () => ({ rpc }));

const repository = (id: string) => ({ id, name: `repo-${id}`, path: `/w/${id}` });

/** The repositories as saved, which `reorder` changes. */
let saved: ReturnType<typeof repository>[];

function renderTabs() {
  const root = createRootRoute({ component: Tabs });
  const router = createRouter({
    routeTree: root.addChildren([
      createRoute({ getParentRoute: () => root, path: "/" }),
      createRoute({ getParentRoute: () => root, path: "/$repoId" }),
    ]),
    history: createMemoryHistory({ initialEntries: ["/a"] }),
  });
  const client = new QueryClient({ defaultOptions: queryClient.getDefaultOptions() });
  render(() => (
    <QueryClientProvider client={client}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  ));
}

/** The repositories' tabs' names, left to right. */
const tabNames = () =>
  screen
    .getAllByRole("link")
    .map((link) => link.textContent)
    .filter((name) => name?.startsWith("repo-"));

/** Where `el` is from the tabs' left: 100px a tab, as jsdom has no layout. */
function left(el: HTMLElement) {
  const tabs = screen.getByRole("link", { name: "Home" }).parentElement!.parentElement!;
  let child: HTMLElement | null = el;
  while (child && child.parentElement !== tabs) child = child.parentElement;
  return child ? [...tabs.children].indexOf(child) * 100 : 0;
}

/** The tab's box, which is what's dragged. */
const tab = async (name: string) => (await screen.findByRole("link", { name })).parentElement!;

beforeEach(() => {
  // jsdom has none: the underline's placed where the tabs are once, which is all these need.
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  saved = ["a", "b", "c"].map(repository);
  rpc.repository.list.mockImplementation(async () => saved);
  rpc.repository.reorder.mockImplementation(async ({ ids }: { ids: string[] }) => {
    saved = ids.map(repository);
  });
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("Tabs", () => {
  it("moves a tab with Alt+Shift and the arrow keys, keeping it focused", async () => {
    const user = userEvent.setup();
    renderTabs();

    (await screen.findByRole("link", { name: "repo-a" })).focus();
    await user.keyboard("{Alt>}{Shift>}{ArrowRight}{/Shift}{/Alt}");

    expect(tabNames()).toEqual(["repo-b", "repo-a", "repo-c"]);
    expect(screen.getByRole("link", { name: "repo-a" })).toHaveFocus();
    await vi.waitFor(() => expect(rpc.repository.list).toHaveBeenCalledTimes(2));
    expect(rpc.repository.reorder).toHaveBeenCalledWith({ ids: ["b", "a", "c"] });
    expect(tabNames()).toEqual(["repo-b", "repo-a", "repo-c"]);
    expect(screen.getByRole("link", { name: "repo-a" })).toHaveFocus();
  });

  it("moves a dragged tab past the middle of each tab it's dragged over", async () => {
    // jsdom has no layout or pointer capture: each tab is 100px wide, the home tab first.
    HTMLElement.prototype.setPointerCapture = () => undefined;
    HTMLElement.prototype.releasePointerCapture = () => undefined;
    vi.spyOn(HTMLElement.prototype, "offsetLeft", "get").mockImplementation(function (
      this: HTMLElement,
    ) {
      return left(this);
    });
    vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(100);
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (
      this: HTMLElement,
    ) {
      return new DOMRect(left(this), 0, 100, 48);
    });
    renderTabs();

    // Grabbed in its middle, and dragged until its middle's just past the next tab's.
    const a = await tab("repo-a");
    fireEvent.pointerDown(a, { button: 0, clientX: 150, pointerId: 1 });
    fireEvent.pointerMove(a, { clientX: 240, pointerId: 1 });
    expect(tabNames()).toEqual(["repo-a", "repo-b", "repo-c"]);
    fireEvent.pointerMove(a, { clientX: 260, pointerId: 1 });
    expect(tabNames()).toEqual(["repo-b", "repo-a", "repo-c"]);
    expect(rpc.repository.reorder).not.toHaveBeenCalled();

    fireEvent.pointerUp(a, { pointerId: 1 });
    await vi.waitFor(() => expect(rpc.repository.list).toHaveBeenCalledTimes(2));
    expect(rpc.repository.reorder).toHaveBeenCalledWith({ ids: ["b", "a", "c"] });
    expect(tabNames()).toEqual(["repo-b", "repo-a", "repo-c"]);
  });

  it("puts the tabs back, and says why, when the order can't be saved", async () => {
    const user = userEvent.setup();
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    rpc.repository.reorder.mockRejectedValue(new Error("The database is locked."));
    renderTabs();

    (await screen.findByRole("link", { name: "repo-a" })).focus();
    await user.keyboard("{Alt>}{Shift>}{ArrowRight}{/Shift}{/Alt}");

    expect(await screen.findByText("The database is locked.")).toBeInTheDocument();
    expect(screen.getByText("Move repo-a")).toBeInTheDocument();
    expect(tabNames()).toEqual(["repo-a", "repo-b", "repo-c"]);
  });
});
