import type { ChangedFile, Uncommitted } from "@gitto/git/types";
import { fireEvent, render, screen } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { createEffect, on } from "solid-js";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { DiffSource } from "@/git/diff-source";
import { gitKeys } from "@/git/queries/keys";

import { FileDiffView } from "./file-diff-view";
import type { EditSession } from "./patch-viewer";

const rpc = vi.hoisted(() => ({
  git: {
    diff: {
      unstagedFilePatch: vi.fn(async ({ path }: { path: string }) => changedLine(path, "old")),
      stagedFilePatch: vi.fn(async ({ path }: { path: string }) => changedLine(path, "staged")),
      saveWorkingTreeFile: vi.fn(async () => ({ version: "v1" })),
    },
    status: { get: vi.fn(async () => uncommitted) },
  },
}));
vi.mock("@/lib/rpc", () => ({ rpc }));

/** What the mocked viewer was given while editing, to type into it. */
const viewer = vi.hoisted(() => ({ onEdit: undefined as ((text: string) => void) | undefined }));
const discard = vi.fn();

// The viewer needs a layout, workers and the library's editor, which jsdom doesn't have: this one
// starts editing whenever it's asked to, of version `v0`, and types with `type`.
vi.mock("./patch-viewer", () => ({
  default: (props: {
    patch: string;
    editing?: boolean;
    onShown: () => void;
    onEditing?: (session: EditSession | undefined) => void;
    onEdit?: (text: string) => void;
  }) => {
    createEffect(on(() => props.patch, props.onShown));
    createEffect(
      on(
        () => props.editing,
        (editing) => {
          viewer.onEdit = editing ? props.onEdit : undefined;
          props.onEditing?.(
            editing
              ? { version: "v0", text: "old\n", discard, hasSelection: () => false }
              : undefined,
          );
        },
        { defer: true },
      ),
    );
    return <pre>{props.patch}</pre>;
  },
  preparePatch: async () => undefined,
}));

function changedLine(path: string, line: string) {
  return `diff --git a/${path} b/${path}\nindex 1..2 100644\n--- a/${path}\n+++ b/${path}\n@@ -1 +1 @@\n-before\n+${line}\n`;
}

function file(path: string, status: ChangedFile["status"] = "modified"): ChangedFile {
  return { path, status, origPath: null, additions: 1, deletions: 1 };
}

let uncommitted: Uncommitted;
function setChanges(unstaged: ChangedFile[], staged: ChangedFile[] = []) {
  uncommitted = {
    head: { kind: "branch", name: "main", sha: "a1" },
    upstream: null,
    ahead: 0,
    behind: 0,
    counts: { files: 1, staged: staged.length, unstaged: unstaged.length, conflicted: 0 },
    changes: { staged, unstaged, uncounted: false },
    version: String(Math.random()),
  };
}

/** Saving fails as the main process says the file changed on disk. */
const changedOnDisk = () =>
  Object.assign(new Error("a.txt changed on disk since you started editing it."), {
    code: "CONFLICT",
  });

function renderView(
  shown: ChangedFile,
  source: DiffSource = { kind: "unstaged" },
  handlers: { onClose?: () => void } = {},
) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  client.setQueryDefaults(gitKeys.all, { staleTime: Infinity });
  let guard: (() => Promise<boolean>) | undefined;
  render(() => (
    <QueryClientProvider client={client}>
      <FileDiffView
        repositoryId="repo"
        source={source}
        file={shown}
        onOpen={() => {}}
        onClose={handlers.onClose ?? (() => {})}
        onGuard={(next) => (guard = next)}
      />
    </QueryClientProvider>
  ));
  return { client, leave: () => guard?.() ?? Promise.resolve(true) };
}

async function startEditing() {
  await userEvent.click(await screen.findByRole("button", { name: "Edit file" }));
  await vi.waitFor(() => expect(viewer.onEdit).toBeDefined());
}

describe("editing an unstaged file", () => {
  beforeEach(() => {
    setChanges([file("a.txt")]);
    rpc.git.diff.saveWorkingTreeFile.mockReset();
    rpc.git.diff.saveWorkingTreeFile.mockResolvedValue({ version: "v1" });
    discard.mockClear();
  });

  it("saves the edits once the typing stops, over the version they're of", async () => {
    renderView(file("a.txt"));
    await screen.findByText(/\+old/);
    await startEditing();

    viewer.onEdit!("new\n");
    expect(await screen.findByText("Saving…")).toBeInTheDocument();
    await vi.waitFor(() =>
      expect(rpc.git.diff.saveWorkingTreeFile).toHaveBeenCalledWith({
        repositoryId: "repo",
        path: "a.txt",
        contents: "new\n",
        version: "v0",
        overwrite: false,
      }),
    );
    expect(await screen.findByText("Saved")).toBeInTheDocument();
  });

  it("is only offered for a working tree file that's there", async () => {
    setChanges([file("gone.txt", "deleted")], [file("a.txt")]);
    renderView(file("a.txt"), { kind: "staged" });
    await screen.findByText(/\+staged/);
    expect(screen.queryByRole("button", { name: "Edit file" })).not.toBeInTheDocument();
  });

  it("holds the changes refetched while editing, and shows them once it stops", async () => {
    const { client } = renderView(file("a.txt"));
    await screen.findByText(/\+old/);
    await startEditing();

    rpc.git.diff.unstagedFilePatch.mockResolvedValueOnce(changedLine("a.txt", "saved"));
    await client.invalidateQueries({ queryKey: gitKeys.uncommitted("repo") });
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(screen.getByText(/\+old/)).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Stop editing (Esc)" }));
    expect(await screen.findByText(/\+saved/)).toBeInTheDocument();
  });

  it("says when the file changed on disk, and saves over it when asked", async () => {
    rpc.git.diff.saveWorkingTreeFile.mockRejectedValueOnce(changedOnDisk());
    renderView(file("a.txt"));
    await screen.findByText(/\+old/);
    await startEditing();

    viewer.onEdit!("mine\n");
    expect(await screen.findByText(/changed on disk since you started editing it/)).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Overwrite" }));
    expect(rpc.git.diff.saveWorkingTreeFile).toHaveBeenLastCalledWith(
      expect.objectContaining({ contents: "mine\n", version: "v0", overwrite: true }),
    );
    await vi.waitFor(() => expect(screen.queryByText(/changed on disk/)).not.toBeInTheDocument());
  });

  it("asks before leaving edits that couldn't be saved: Cancel stays, Discard drops them", async () => {
    rpc.git.diff.saveWorkingTreeFile.mockRejectedValue(changedOnDisk());
    const { leave } = renderView(file("a.txt"));
    await screen.findByText(/\+old/);
    await startEditing();
    viewer.onEdit!("mine\n");
    await screen.findByText(/changed on disk/);

    const staying = leave();
    await userEvent.click(await screen.findByRole("button", { name: "Cancel" }));
    expect(await staying).toBe(false);
    expect(await screen.findByRole("button", { name: "Stop editing (Esc)" })).toBeInTheDocument();

    const leaving = leave();
    await userEvent.click(await screen.findByRole("button", { name: "Discard" }));
    expect(await leaving).toBe(true);
    expect(discard).toHaveBeenCalledOnce();
    expect(await screen.findByRole("button", { name: "Edit file" })).toBeInTheDocument();
  });

  it("leaves edit mode with Esc, and closes the file with the next", async () => {
    const onClose = vi.fn();
    renderView(file("a.txt"), { kind: "unstaged" }, { onClose });
    await screen.findByText(/\+old/);
    await startEditing();

    fireEvent.keyDown(document.body, { key: "Escape" });
    expect(await screen.findByRole("button", { name: "Edit file" })).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.keyDown(document.body, { key: "Escape" });
    expect(onClose).toHaveBeenCalledOnce();
  });
});
