import type { ChangedFile, Conflict, Uncommitted } from "@gitto/git/types";
import { fireEvent, render, screen } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { createEffect, on } from "solid-js";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { DiffSource } from "@/git/diff-source";
import { gitKeys } from "@/git/queries/keys";

import type { ConflictCommands, ConflictProgress, DiskFile } from "./conflict-viewer";
import { FileDiffView } from "./file-diff-view";
import type { EditSession } from "./viewer-editing";

const rpc = vi.hoisted(() => ({
  git: {
    diff: {
      unstagedFilePatch: vi.fn(async () => ""),
      saveWorkingTreeFile: vi.fn(async () => ({ version: "v2" })),
    },
    status: { get: vi.fn(async () => uncommitted) },
    conflicts: {
      get: vi.fn(async ({ path }: { path: string }) => conflicts[path]!),
      keep: vi.fn<(input: unknown) => Promise<void>>(async () => undefined),
      markResolved: vi.fn<(input: unknown) => Promise<void>>(async () => undefined),
    },
  },
}));
vi.mock("@/lib/rpc", () => ({ rpc }));

/** What the mocked viewer was last given, and what it tells: the conflicts it has left. */
const viewer = vi.hoisted(() => ({
  props: undefined as ViewerProps | undefined,
  left: 2,
  commands: { next: vi.fn(), previous: vi.fn(), resolve: vi.fn() },
}));

interface ViewerProps {
  fileKey: string;
  disk: DiskFile | undefined;
  editing: boolean;
  save: (contents: string, version: string) => Promise<string>;
  onEditing: (session: EditSession | undefined) => void;
  onProgress: (progress: ConflictProgress) => void;
  onCommands: (commands: ConflictCommands | undefined) => void;
  onError: (message: string | undefined) => void;
  onShown: () => void;
}

// The viewer needs a layout and workers, which jsdom doesn't have: this one shows the text it's
// given, says it has `viewer.left` conflicts left, and starts editing whenever it's asked to.
vi.mock("./conflict-viewer", () => ({
  default: (props: ViewerProps) => {
    viewer.props = props;
    props.onCommands(viewer.commands);
    createEffect(
      on(
        () => props.disk,
        (disk) => {
          if (!disk) return;
          props.onProgress({
            left: viewer.left,
            current: viewer.left ? 1 : undefined,
            saving: false,
            version: disk.version,
            problem: undefined,
          });
          props.onShown();
        },
      ),
    );
    createEffect(
      on(
        () => props.editing,
        (editing) =>
          props.onEditing(
            editing ? { version: "v1", discard: () => {}, hasSelection: () => false } : undefined,
          ),
        { defer: true },
      ),
    );
    return <pre>{props.disk?.contents}</pre>;
  },
  prepareConflicts: () => undefined,
}));

const MARKERS = "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> side\n";

function side(oid: string) {
  return { mode: "100644", oid };
}

/** The conflicts the main process reads, by path. */
let conflicts: Record<string, Conflict>;

function textConflict(contents = MARKERS, version = "v1"): Conflict {
  return {
    base: side("b1"),
    ours: side("o1"),
    theirs: side("t1"),
    text: { contents, version },
    version,
    binary: false,
    unreadable: null,
  };
}

function conflicted(path: string): ChangedFile {
  return { path, status: "conflicted", origPath: null, additions: null, deletions: null };
}

let uncommitted: Uncommitted;
function setChanges(unstaged: ChangedFile[]) {
  uncommitted = {
    head: { kind: "branch", name: "main", sha: "a1" },
    upstream: null,
    ahead: 0,
    behind: 0,
    counts: { files: unstaged.length, staged: 0, unstaged: 0, conflicted: unstaged.length },
    changes: { staged: [], unstaged, uncounted: false, markerFree: [] },
    version: String(Math.random()),
  };
}

const UNSTAGED: DiffSource = { kind: "unstaged" };

function renderView(shown: ChangedFile, onOpen = vi.fn()) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  client.setQueryDefaults(gitKeys.all, { staleTime: Infinity });
  render(() => (
    <QueryClientProvider client={client}>
      <FileDiffView
        repositoryId="repo"
        source={UNSTAGED}
        file={shown}
        onOpen={onOpen}
        onClose={() => {}}
      />
    </QueryClientProvider>
  ));
  return { client, onOpen };
}

describe("a file with text conflicts", () => {
  beforeEach(() => {
    viewer.left = 2;
    viewer.props = undefined;
    for (const command of Object.values(viewer.commands)) command.mockClear();
    conflicts = { "a.txt": textConflict(), "b.txt": textConflict() };
    setChanges([conflicted("a.txt"), conflicted("b.txt")]);
    rpc.git.conflicts.markResolved.mockClear();
    rpc.git.conflicts.keep.mockClear();
    rpc.git.diff.unstagedFilePatch.mockClear();
  });

  it("shows its conflicts, rather than a patch, and how many are left", async () => {
    renderView(conflicted("a.txt"));

    expect(await screen.findByText(/<<<<<<< HEAD/)).toBeInTheDocument();
    expect(screen.getByText("2 conflicts left")).toBeInTheDocument();
    expect(screen.getByText("Changed on both sides")).toBeInTheDocument();
    expect(rpc.git.diff.unstagedFilePatch).not.toHaveBeenCalled();
    // Ours above theirs, one way only.
    expect(screen.queryByRole("button", { name: "Side by side" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Stage file" })).not.toBeInTheDocument();
  });

  it("isn't marked resolved while conflicts are left", async () => {
    renderView(conflicted("a.txt"));
    await screen.findByText("2 conflicts left");

    expect(screen.getByRole("button", { name: "Mark resolved" })).toBeDisabled();
    fireEvent.keyDown(window, { key: "Enter", metaKey: true });
    expect(rpc.git.conflicts.markResolved).not.toHaveBeenCalled();
  });

  it("is marked resolved once none are, then the next conflicted file opens", async () => {
    viewer.left = 0;
    const { onOpen } = renderView(conflicted("a.txt"));
    await screen.findByText("No conflicts left");

    await userEvent.click(screen.getByRole("button", { name: "Mark resolved" }));
    expect(rpc.git.conflicts.markResolved).toHaveBeenCalledWith({
      repositoryId: "repo",
      path: "a.txt",
      version: "v1",
    });
    expect(onOpen).toHaveBeenCalledWith(UNSTAGED, conflicted("b.txt"));
  });

  it("opens its staged changes once it was the last conflicted file", async () => {
    viewer.left = 0;
    setChanges([conflicted("a.txt")]);
    const { onOpen } = renderView(conflicted("a.txt"));
    await screen.findByText("No conflicts left");

    fireEvent.keyDown(window, { key: "Enter", ctrlKey: true });
    await vi.waitFor(() =>
      expect(onOpen).toHaveBeenCalledWith(
        { kind: "staged" },
        { ...conflicted("a.txt"), status: "modified" },
      ),
    );
  });

  it("says why it couldn't be marked resolved", async () => {
    viewer.left = 0;
    rpc.git.conflicts.markResolved.mockRejectedValueOnce(
      new Error("a.txt still has conflict markers. Resolve its conflicts, then mark it resolved."),
    );
    const { onOpen } = renderView(conflicted("a.txt"));
    await screen.findByText("No conflicts left");

    await userEvent.click(screen.getByRole("button", { name: "Mark resolved" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("a.txt still has conflict markers.");
    expect(onOpen).not.toHaveBeenCalled();
  });

  it("steps through its conflicts and resolves them with the keyboard", async () => {
    const { onOpen } = renderView(conflicted("a.txt"));
    await screen.findByText("2 conflicts left");

    await userEvent.keyboard("]");
    expect(viewer.commands.next).toHaveBeenCalled();
    await userEvent.keyboard("[[");
    expect(viewer.commands.previous).toHaveBeenCalled();
    await userEvent.keyboard("o");
    expect(viewer.commands.resolve).toHaveBeenLastCalledWith("current");
    await userEvent.keyboard("t");
    expect(viewer.commands.resolve).toHaveBeenLastCalledWith("incoming");
    await userEvent.keyboard("b");
    expect(viewer.commands.resolve).toHaveBeenLastCalledWith("both");
    await userEvent.keyboard("n");
    expect(onOpen).toHaveBeenCalledWith(UNSTAGED, conflicted("b.txt"));
  });

  it("writes a resolved conflict over the version it was read at", async () => {
    renderView(conflicted("a.txt"));
    await screen.findByText("2 conflicts left");

    expect(await viewer.props!.save("ours\n", "v1")).toBe("v2");
    expect(rpc.git.diff.saveWorkingTreeFile).toHaveBeenCalledWith({
      repositoryId: "repo",
      path: "a.txt",
      contents: "ours\n",
      version: "v1",
      overwrite: false,
    });
  });

  it("says why a conflict couldn't be resolved", async () => {
    renderView(conflicted("a.txt"));
    await screen.findByText("2 conflicts left");

    viewer.props!.onError("a.txt was changed on disk since you started editing it.");
    expect(await screen.findByRole("alert")).toHaveTextContent("a.txt was changed on disk");
  });

  it("keeps a side whole, of the version shown", async () => {
    const { onOpen } = renderView(conflicted("a.txt"));
    await screen.findByText("2 conflicts left");

    await userEvent.click(screen.getByRole("button", { name: "Keep all of theirs" }));
    expect(rpc.git.conflicts.keep).toHaveBeenCalledWith({
      repositoryId: "repo",
      path: "a.txt",
      side: "theirs",
      sides: { base: side("b1"), ours: side("o1"), theirs: side("t1") },
      version: "v1",
    });
    expect(onOpen).toHaveBeenCalledWith(UNSTAGED, conflicted("b.txt"));
  });

  it("is edited by hand, and read again once that stops, before it's shown", async () => {
    const { client } = renderView(conflicted("a.txt"));
    await screen.findByText("2 conflicts left");

    await userEvent.click(screen.getByRole("button", { name: "Edit file" }));
    await vi.waitFor(() => expect(viewer.props!.editing).toBe(true));
    // What's read while it's edited isn't for the viewer.
    expect(viewer.props!.disk).toBeUndefined();

    conflicts["a.txt"] = textConflict("resolved\n", "v3");
    await userEvent.click(screen.getByRole("button", { name: "Stop editing (Esc)" }));
    await vi.waitFor(() =>
      expect(viewer.props!.disk).toEqual({ contents: "resolved\n", version: "v3" }),
    );
    expect(client.getQueryData(gitKeys.conflict("repo", "a.txt"))).toMatchObject({
      version: "v3",
    });
  });
});

describe("a file that isn't text on both sides", () => {
  beforeEach(() => {
    rpc.git.conflicts.keep.mockClear();
    setChanges([conflicted("gone.txt")]);
  });

  it("says what each side did, and keeps one whole", async () => {
    conflicts = {
      "gone.txt": { ...textConflict("ours\n"), theirs: null },
    };
    const { onOpen } = renderView(conflicted("gone.txt"));

    expect(await screen.findByText("Deleted in theirs, changed in ours")).toBeInTheDocument();
    expect(screen.getByText("Deleted")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Delete, as theirs does" }));
    expect(rpc.git.conflicts.keep).toHaveBeenCalledWith(
      expect.objectContaining({ path: "gone.txt", side: "theirs", version: "v1" }),
    );
    // It was the last one: its staged changes, its deletion, are next.
    expect(onOpen).toHaveBeenCalledWith(
      { kind: "staged" },
      expect.objectContaining({ path: "gone.txt" }),
    );
    expect(screen.queryByRole("button", { name: "Mark resolved" })).not.toBeInTheDocument();
  });

  it("says a binary file can't be shown as text", async () => {
    conflicts = {
      "gone.txt": {
        ...textConflict(),
        text: null,
        binary: true,
        unreadable: "This file is binary.",
      },
    };
    renderView(conflicted("gone.txt"));

    expect(await screen.findByText("This file is binary.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Keep ours" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Keep theirs" })).toBeEnabled();
  });
});
