import type { Uncommitted } from "@gitto/git/types";
import { render, screen } from "@solidjs/testing-library";
import { onlineManager, QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { createSignal } from "solid-js";
import { afterEach, describe, expect, it, vi } from "vitest";

import { gitKeys } from "@/git/queries/keys";
import { queryClient } from "@/lib/query-client";

import { PullButton } from "./pull-button";

const rpc = vi.hoisted(() => ({
  git: { status: { get: vi.fn() }, remote: { pull: vi.fn() } },
}));

vi.mock("@/lib/rpc", () => ({ rpc }));

const status: Uncommitted = {
  head: { kind: "branch", name: "main", sha: "abc" },
  upstream: "origin/main",
  ahead: 0,
  behind: 2,
  counts: { files: 0, staged: 0, unstaged: 0, conflicted: 0 },
  changes: { staged: [], unstaged: [], uncounted: false, markerFree: [] },
  version: "v1",
};

/** The pull button, once the status has loaded: until then it's a disabled placeholder. */
async function loadedButton(title: string) {
  await vi.waitFor(() =>
    expect(screen.getByRole("button", { name: "Pull" })).toHaveAttribute("title", title),
  );
  return screen.getByRole("button", { name: "Pull" });
}

/** Renders the pull button; `switchTo` shows another repository's. */
function renderButton() {
  const client = new QueryClient({ defaultOptions: queryClient.getDefaultOptions() });
  const [repositoryId, setRepositoryId] = createSignal("repo");
  render(() => (
    <QueryClientProvider client={client}>
      <PullButton repositoryId={repositoryId()} />
    </QueryClientProvider>
  ));
  return { client, switchTo: setRepositoryId };
}

afterEach(() => {
  vi.resetAllMocks();
});

describe("the pull button", () => {
  it("pulls, and shows how many commits there are to pull", async () => {
    const user = userEvent.setup();
    rpc.git.status.get.mockResolvedValue(status);
    rpc.git.remote.pull.mockResolvedValue(undefined);
    renderButton();

    const button = await loadedButton("Pull 2 commits from origin/main");
    expect(button).toBeEnabled();
    expect(button).toHaveTextContent("2");

    await user.click(button);
    expect(rpc.git.remote.pull).toHaveBeenCalledWith({ repositoryId: "repo" });
    // The status is reloaded afterwards.
    await vi.waitFor(() => expect(rpc.git.status.get).toHaveBeenCalledTimes(2));
  });

  it("says why a pull failed", async () => {
    const user = userEvent.setup();
    rpc.git.status.get.mockResolvedValue(status);
    rpc.git.remote.pull.mockRejectedValue(new Error("Pulling origin/main caused conflicts."));
    renderButton();

    await user.click(await loadedButton("Pull 2 commits from origin/main"));

    expect(await screen.findByText("Pulling origin/main caused conflicts.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Dismiss" }));
    await vi.waitFor(() =>
      expect(screen.queryByText("Pulling origin/main caused conflicts.")).not.toBeInTheDocument(),
    );
  });

  it("is disabled without an upstream to pull from", async () => {
    rpc.git.status.get.mockResolvedValue({ ...status, upstream: null, behind: 0 });
    renderButton();

    expect(await loadedButton("main doesn't track a remote branch.")).toBeDisabled();
  });

  it("keeps a pull to the repository it was started in", async () => {
    const user = userEvent.setup();
    rpc.git.status.get.mockResolvedValue(status);
    let fail!: (error: Error) => void;
    rpc.git.remote.pull.mockReturnValue(new Promise((_, reject) => (fail = reject)));
    const { client, switchTo } = renderButton();

    await user.click(await loadedButton("Pull 2 commits from origin/main"));
    expect(screen.getByRole("button", { name: "Pull" })).toHaveAttribute("aria-busy", "true");

    // Another repository's pull button isn't busy with it…
    switchTo("other");
    const other = await loadedButton("Pull 2 commits from origin/main");
    expect(other).toBeEnabled();
    expect(other).not.toHaveAttribute("aria-busy", "true");

    // …and doesn't show why it failed, which the repository that was pulled is refreshed for.
    fail(new Error("Pulling origin/main caused conflicts."));
    await vi.waitFor(() =>
      expect(client.getQueryState(gitKeys.status("repo"))?.isInvalidated).toBe(true),
    );
    expect(client.getQueryState(gitKeys.status("other"))?.isInvalidated).toBe(false);
    expect(screen.queryByText("Pulling origin/main caused conflicts.")).not.toBeInTheDocument();

    // Back in that repository, it does, without taking the focus from what the user's doing.
    const input = document.createElement("input");
    document.body.append(input);
    input.focus();
    switchTo("repo");
    expect(await screen.findByText("Pulling origin/main caused conflicts.")).toBeInTheDocument();
    expect(input).toHaveFocus();
    input.remove();
  });

  it("stays busy when switching away from a running pull and back", async () => {
    const user = userEvent.setup();
    rpc.git.status.get.mockResolvedValue(status);
    rpc.git.remote.pull.mockReturnValue(new Promise(() => undefined));
    const { switchTo } = renderButton();

    await user.click(await loadedButton("Pull 2 commits from origin/main"));
    switchTo("other");
    await loadedButton("Pull 2 commits from origin/main");
    switchTo("repo");
    await vi.waitFor(() =>
      expect(screen.getByRole("button", { name: "Pull" })).toHaveAttribute("aria-busy", "true"),
    );
    expect(screen.getByRole("button", { name: "Pull" })).toBeDisabled();
    expect(rpc.git.remote.pull).toHaveBeenCalledTimes(1);
  });

  it("pulls while the browser thinks it's offline", async () => {
    const user = userEvent.setup();
    rpc.git.status.get.mockResolvedValue(status);
    rpc.git.remote.pull.mockResolvedValue(undefined);
    renderButton();
    const button = await loadedButton("Pull 2 commits from origin/main");
    onlineManager.setOnline(false);
    try {
      await user.click(button);
      await vi.waitFor(() => expect(rpc.git.remote.pull).toHaveBeenCalled());
    } finally {
      onlineManager.setOnline(true);
    }
  });
});
