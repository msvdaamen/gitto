import type { ChangedFile, Operation, Uncommitted } from "@gitto/git/types";
import { render, screen } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { gitKeys } from "@/git/queries/keys";

import { OperationBar } from "./operation-bar";

const rpc = vi.hoisted(() => ({
  git: {
    status: { get: vi.fn(async () => uncommitted) },
    operation: {
      get: vi.fn(async (): Promise<Operation | null> => operation),
      continue: vi.fn<(input: unknown) => Promise<void>>(async () => undefined),
      abort: vi.fn<(input: unknown) => Promise<void>>(async () => undefined),
    },
  },
}));
vi.mock("@/lib/rpc", () => ({ rpc }));

let operation: Operation | null;
let uncommitted: Uncommitted;

function conflicted(path: string): ChangedFile {
  return { path, status: "conflicted", origPath: null, additions: null, deletions: null };
}

function setConflicts(paths: string[], markerFree: string[] = []) {
  uncommitted = {
    head: { kind: "branch", name: "main", sha: "a1" },
    upstream: null,
    ahead: 0,
    behind: 0,
    counts: { files: paths.length, staged: 0, unstaged: 0, conflicted: paths.length },
    changes: { staged: [], unstaged: paths.map(conflicted), uncounted: false, markerFree },
    version: String(Math.random()),
  };
}

function renderBar(beforeAbort = async () => true) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  client.setQueryDefaults(gitKeys.all, { staleTime: Infinity });
  const onResolve = vi.fn();
  render(() => (
    <QueryClientProvider client={client}>
      <OperationBar repositoryId="repo" onResolve={onResolve} beforeAbort={beforeAbort} />
    </QueryClientProvider>
  ));
  return { onResolve };
}

describe("the operation bar", () => {
  beforeEach(() => {
    rpc.git.operation.continue.mockClear();
    rpc.git.operation.abort.mockClear();
    operation = { kind: "merge", merging: "feature", into: "main" };
  });

  it("isn't there without an operation or conflicts", async () => {
    operation = null;
    setConflicts([]);
    renderBar();
    await vi.waitFor(() => expect(rpc.git.operation.get).toHaveBeenCalled());
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("says what's under way, and only continues once nothing's conflicted", async () => {
    setConflicts(["a.txt", "b.txt"], ["b.txt"]);
    const { onResolve } = renderBar();

    expect(await screen.findByText("Merging feature into main")).toBeInTheDocument();
    expect(
      screen.getByText("2 files have conflicts, 1 ready to mark resolved"),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Commit merge" })).toBeDisabled();
    // The one with markers left first.
    await userEvent.click(screen.getByRole("button", { name: "Resolve conflicts" }));
    expect(onResolve).toHaveBeenCalledWith(conflicted("a.txt"));
  });

  it("continues once every conflict is resolved", async () => {
    operation = { kind: "rebase", branch: "feature", onto: "main", steps: { step: 3, total: 7 } };
    setConflicts([]);
    renderBar();

    expect(await screen.findByText("Rebasing feature onto main")).toBeInTheDocument();
    expect(screen.getByText("3/7")).toBeInTheDocument();
    expect(screen.getByText("All conflicts resolved")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Continue" }));
    expect(rpc.git.operation.continue).toHaveBeenCalledWith({
      repositoryId: "repo",
      kind: "rebase",
    });
  });

  it("asks before aborting, which throws away what's resolved", async () => {
    setConflicts(["a.txt"]);
    const beforeAbort = vi.fn(async () => true);
    renderBar(beforeAbort);

    await userEvent.click(await screen.findByRole("button", { name: "Abort" }));
    expect(await screen.findByRole("alertdialog")).toHaveTextContent("Abort the merge?");
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(rpc.git.operation.abort).not.toHaveBeenCalled();

    // Once the dialog has gone, and the page is no longer hidden behind it.
    await userEvent.click(await screen.findByRole("button", { name: "Abort" }));
    const dialog = await screen.findByRole("alertdialog");
    await userEvent.click(dialog.querySelector("button:last-child")!);
    await vi.waitFor(() =>
      expect(rpc.git.operation.abort).toHaveBeenCalledWith({ repositoryId: "repo", kind: "merge" }),
    );
    expect(beforeAbort).toHaveBeenCalled();
  });

  it("doesn't abort while a file's edits can't be saved", async () => {
    setConflicts(["a.txt"]);
    renderBar(async () => false);

    await userEvent.click(await screen.findByRole("button", { name: "Abort" }));
    const dialog = await screen.findByRole("alertdialog");
    await userEvent.click(dialog.querySelector("button:last-child")!);
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(rpc.git.operation.abort).not.toHaveBeenCalled();
  });

  it("shows the conflicts a popped stash left, without an operation to continue", async () => {
    operation = null;
    setConflicts(["a.txt"]);
    renderBar();

    expect(await screen.findByText("Conflicts")).toBeInTheDocument();
    expect(
      screen.getByText("1 file has conflicts. Resolve them, then commit."),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Abort" })).not.toBeInTheDocument();
  });
});
