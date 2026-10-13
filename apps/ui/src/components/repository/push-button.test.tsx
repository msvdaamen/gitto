import type { PushTarget, Uncommitted } from "@gitto/git/types";
import { render, screen } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { queryClient } from "@/lib/query-client";

import { PushButton } from "./push-button";

const rpc = vi.hoisted(() => ({
  git: {
    status: { get: vi.fn() },
    remote: { push: vi.fn(), pushTarget: vi.fn(), pull: vi.fn() },
  },
}));

vi.mock("@/lib/rpc", () => ({ rpc }));

const target: PushTarget = {
  kind: "remote",
  name: "origin/main",
  isNew: false,
  ahead: 2,
  pullsFrom: true,
};

const status: Uncommitted = {
  head: { kind: "branch", name: "main", sha: "abc" },
  upstream: "origin/main",
  ahead: 2,
  behind: 1,
  counts: { files: 0, staged: 0, unstaged: 0, conflicted: 0 },
  changes: { staged: [], unstaged: [], uncounted: false, markerFree: [] },
  version: "v1",
};

const REWRITTEN = "origin/main still has main's commits from before a rebase or amend.";
const BEHIND = "origin/main has commits you haven't pulled yet.";

/** A push turned down as the renderer gets it (see `pushRejection`). */
function rejected(kind: "rewritten" | "behind") {
  return kind === "rewritten"
    ? Object.assign(new Error(REWRITTEN), { code: "PUSH_REWRITTEN" })
    : Object.assign(new Error(BEHIND), { code: "PUSH_BEHIND" });
}

/** The push button, once where it pushes has loaded: until then it's a disabled placeholder. */
async function loadedButton(title: string) {
  await vi.waitFor(() =>
    expect(screen.getByRole("button", { name: "Push" })).toHaveAttribute("title", title),
  );
  return screen.getByRole("button", { name: "Push" });
}

function renderButton() {
  const client = new QueryClient({ defaultOptions: queryClient.getDefaultOptions() });
  render(() => (
    <QueryClientProvider client={client}>
      <PushButton repositoryId="repo" />
    </QueryClientProvider>
  ));
}

beforeEach(() => {
  rpc.git.status.get.mockResolvedValue(status);
  rpc.git.remote.pushTarget.mockResolvedValue(target);
});

afterEach(() => {
  vi.resetAllMocks();
});

describe("the push button", () => {
  it("pushes, and shows how many commits there are to push", async () => {
    const user = userEvent.setup();
    rpc.git.remote.push.mockResolvedValue(undefined);
    renderButton();

    const button = await loadedButton("Push 2 commits to origin/main");
    expect(button).toHaveTextContent("2");
    await user.click(button);
    expect(rpc.git.remote.push).toHaveBeenCalledWith({ repositoryId: "repo", force: false });
    // Where it pushes, and how many commits, is reloaded afterwards.
    await vi.waitFor(() => expect(rpc.git.remote.pushTarget).toHaveBeenCalledTimes(2));
  });

  it("offers to force-push when the remote has the commits from before a rebase", async () => {
    const user = userEvent.setup();
    rpc.git.remote.push
      .mockRejectedValueOnce(rejected("rewritten"))
      .mockResolvedValueOnce(undefined);
    renderButton();

    await user.click(await loadedButton("Push 2 commits to origin/main"));
    const dialog = await screen.findByRole("alertdialog", { name: "Push rejected" });
    expect(dialog).toHaveTextContent(`${REWRITTEN} Force-push to replace them.`);
    // Pulling is there too, but force-pushing is what's suggested.
    expect(screen.getByRole("button", { name: "Pull" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Force push" }));
    expect(rpc.git.remote.push).toHaveBeenLastCalledWith({ repositoryId: "repo", force: true });
    await vi.waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
  });

  it("offers to pull, and not to force-push, over someone else's commits", async () => {
    const user = userEvent.setup();
    rpc.git.remote.push.mockRejectedValue(rejected("behind"));
    rpc.git.remote.pull.mockResolvedValue(undefined);
    renderButton();

    await user.click(await loadedButton("Push 2 commits to origin/main"));
    const dialog = await screen.findByRole("alertdialog", { name: "Push rejected" });
    expect(dialog).toHaveTextContent(`${BEHIND} Pull them first.`);
    expect(screen.queryByRole("button", { name: "Force push" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Pull" }));
    expect(rpc.git.remote.pull).toHaveBeenCalledWith({ repositoryId: "repo" });
    await vi.waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
    expect(rpc.git.remote.push).toHaveBeenCalledTimes(1);
  });

  it("doesn't offer to pull from another branch than it pushes to", async () => {
    const user = userEvent.setup();
    rpc.git.remote.pushTarget.mockResolvedValue({ ...target, name: "fork/main", pullsFrom: false });
    rpc.git.remote.push.mockRejectedValue(rejected("behind"));
    renderButton();

    await user.click(await loadedButton("Push 2 commits to fork/main"));
    expect(await screen.findByRole("alertdialog")).toHaveTextContent(BEHIND);
    expect(screen.queryByRole("button", { name: "Pull" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Close" }));
    await vi.waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
  });

  it("leaves the remote alone when cancelled", async () => {
    const user = userEvent.setup();
    rpc.git.remote.push.mockRejectedValue(rejected("rewritten"));
    renderButton();

    await user.click(await loadedButton("Push 2 commits to origin/main"));
    await screen.findByRole("alertdialog", { name: "Push rejected" });
    await user.click(screen.getByRole("button", { name: "Cancel" }));

    await vi.waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
    expect(rpc.git.remote.push).toHaveBeenCalledTimes(1);
    expect(rpc.git.remote.pull).not.toHaveBeenCalled();
    // Nor shown as a failure.
    expect(screen.queryByText(REWRITTEN)).not.toBeInTheDocument();
  });

  it("says why a push failed otherwise", async () => {
    const user = userEvent.setup();
    rpc.git.remote.push.mockRejectedValue(new Error("Permission denied (publickey)."));
    renderButton();

    await user.click(await loadedButton("Push 2 commits to origin/main"));
    expect(await screen.findByText("Permission denied (publickey).")).toBeInTheDocument();
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });

  it("says when it pushes to a new remote branch", async () => {
    rpc.git.remote.pushTarget.mockResolvedValue({
      ...target,
      name: "origin/feature",
      isNew: true,
    });
    renderButton();
    expect(await loadedButton("Push to origin/feature, a new remote branch")).toBeEnabled();
  });

  it("is disabled, saying why, when there's nothing to push to", async () => {
    rpc.git.remote.pushTarget.mockResolvedValue({
      kind: "blocked",
      reason: "There's no remote to push to.",
    });
    renderButton();
    expect(await loadedButton("There's no remote to push to.")).toBeDisabled();
  });
});
