import { render, screen } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { createSignal } from "solid-js";
import { describe, expect, it, vi } from "vitest";

import { RepositoryToolbar } from "./toolbar";

const rpc = vi.hoisted(() => {
  /** Settles the fetch that's running, with an error if one's passed. */
  let settle: ((error?: Error) => void) | undefined;
  return {
    settle: (error?: Error) => settle?.(error),
    repository: { list: async () => [] },
    git: {
      // Refetched after a fetch, which only finishes once that's done.
      status: { get: async () => Promise.reject(new Error("no status in this test")) },
      remote: {
        fetch: vi.fn(
          () =>
            new Promise<void>((resolve, reject) => {
              settle = (error) => (error ? reject(error) : resolve());
            }),
        ),
      },
    },
  };
});

vi.mock("@/lib/rpc", () => ({ rpc }));

const button = () => screen.getByRole("button", { name: "Fetch" });

describe("the fetch button", () => {
  it("fetches the repository, and shows that it's running and why it failed", async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const [repositoryId, setRepositoryId] = createSignal("a");
    render(() => (
      <QueryClientProvider client={client}>
        <RepositoryToolbar
          repositoryId={repositoryId()}
          search=""
          onSearch={() => undefined}
          sidebarOpen
          onToggleSidebar={() => undefined}
          detailsOpen
          onToggleDetails={() => undefined}
        />
      </QueryClientProvider>
    ));

    await userEvent.click(button());
    await vi.waitFor(() =>
      expect(rpc.git.remote.fetch).toHaveBeenCalledWith({ repositoryId: "a" }),
    );
    expect(button()).toBeDisabled();
    // Busy, not unavailable: it isn't faded.
    expect(button()).toHaveAttribute("aria-busy", "true");
    expect(button()).not.toHaveClass("opacity-50");

    // Another repository's fetch is its own.
    setRepositoryId("b");
    expect(button()).toBeEnabled();
    setRepositoryId("a");
    expect(button()).toBeDisabled();

    rpc.settle(new Error("Could not resolve host: example.com"));
    await vi.waitFor(() => expect(button()).toBeEnabled());
    expect(button()).toHaveAttribute("title", "Fetch failed: Could not resolve host: example.com");

    await userEvent.click(button());
    expect(button()).toHaveAttribute("title", "Fetch all remotes");
    await vi.waitFor(() => expect(rpc.git.remote.fetch).toHaveBeenCalledTimes(2));
    rpc.settle();
    await vi.waitFor(() => expect(button()).toBeEnabled());
    expect(button()).toHaveAttribute("title", "Fetch all remotes");
  });
});
