import type { ChangedFile, Uncommitted, WorkingTreeFiles } from "@gitto/git/types";
import { render, screen } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { createEffect, createSignal, on, Show, Suspense } from "solid-js";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { DiffSource } from "@/git/diff-source";
import { gitKeys } from "@/git/queries/keys";

import { FileDiffView } from "./file-diff-view";

const rpc = vi.hoisted(() => ({
  git: {
    diff: {
      commitFiles: async () => FILES,
      commitFilePatch: vi.fn(patchOf),
      unstagedFilePatch: vi.fn(patchOf),
      stagedFilePatch: vi.fn(patchOf),
    },
    status: { get: vi.fn(async () => uncommitted) },
  },
}));

vi.mock("@/lib/rpc", () => ({ rpc }));
// The viewer needs a layout and workers, which jsdom doesn't have.
vi.mock("./patch-viewer", () => ({
  default: (props: { patch: string; onShown: () => void }) => {
    if (props.patch === "patch of broken.txt") throw new Error("The patch has no file in it.");
    createEffect(on(() => props.patch, props.onShown));
    return <pre>{props.patch}</pre>;
  },
  preparePatch: async () => undefined,
}));

// Function declarations, so they're hoisted above `vi.hoisted` too.
async function patchOf({ path }: { path: string }) {
  return `patch of ${path}`;
}

function file(path: string, additions: number | null = 1, deletions: number | null = 0) {
  return { path, status: "modified", origPath: null, additions, deletions } as const;
}

const FILES: ChangedFile[] = [file("a.txt"), file("b.txt"), file("c.txt")];

const COMMIT: DiffSource = { kind: "commit", sha: "a1" };
const UNSTAGED: DiffSource = { kind: "unstaged" };

/** What the status says is uncommitted, for the tests of uncommitted changes to set. */
let uncommitted: Uncommitted;

function setChanges(changes: Partial<WorkingTreeFiles>) {
  uncommitted = {
    head: { kind: "branch", name: "main", sha: "a1" },
    upstream: null,
    ahead: 0,
    behind: 0,
    counts: { files: 0, staged: 0, unstaged: 0, conflicted: 0 },
    changes: { staged: [], unstaged: [], uncounted: false, ...changes },
    version: String(Math.random()),
  };
}

/** Closes on Esc like a popover's layer: on the document, marking it handled. */
const popover = (event: KeyboardEvent) => event.preventDefault();

function renderView(
  shown: ChangedFile,
  handlers: Partial<{ onOpen: () => void; onClose: () => void }> = {},
  source = COMMIT,
) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(() => (
    <QueryClientProvider client={client}>
      <FileDiffView
        repositoryId="repo"
        source={source}
        file={shown}
        onOpen={handlers.onOpen ?? (() => {})}
        onClose={handlers.onClose ?? (() => {})}
      />
    </QueryClientProvider>
  ));
  return client;
}

describe("a file's changes", () => {
  afterEach(() => {
    rpc.git.diff.commitFilePatch.mockClear();
    rpc.git.diff.commitFilePatch.mockImplementation(patchOf);
  });

  it("shows the file's patch, a step away from the commit's other files", async () => {
    const onOpen = vi.fn();
    renderView(FILES[1]!, { onOpen });

    expect(await screen.findByText("patch of b.txt")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Next file" }));
    expect(onOpen).toHaveBeenLastCalledWith(COMMIT, FILES[2]);
    await userEvent.click(screen.getByRole("button", { name: "Previous file" }));
    expect(onOpen).toHaveBeenLastCalledWith(COMMIT, FILES[0]);
  });

  it("keeps the last file on show, header and all, until the next one's changes are in", async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const [shown, setShown] = createSignal(FILES[0]!);
    render(() => (
      <QueryClientProvider client={client}>
        <FileDiffView
          repositoryId="repo"
          source={COMMIT}
          file={shown()}
          onOpen={(_, next) => setShown(next)}
          onClose={() => {}}
        />
      </QueryClientProvider>
    ));
    let next!: (patch: string) => void;
    rpc.git.diff.commitFilePatch.mockImplementation(({ path }) =>
      path === "b.txt" ? new Promise<string>((done) => (next = done)) : patchOf({ path }),
    );
    expect(await screen.findByText("patch of a.txt")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Next file" }));
    expect(screen.getByText("patch of a.txt")).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Changes in a.txt" })).toBeInTheDocument();

    next("patch of b.txt");
    expect(await screen.findByText("patch of b.txt")).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Changes in b.txt" })).toBeInTheDocument();
  });

  it("loads the files a step away ahead, once it's on show", async () => {
    renderView(FILES[1]!);

    expect(await screen.findByText("patch of b.txt")).toBeInTheDocument();
    await vi.waitFor(() =>
      expect(
        rpc.git.diff.commitFilePatch.mock.calls.map(([input]) => input.path).toSorted(),
      ).toEqual(["a.txt", "b.txt", "c.txt"]),
    );
  });

  it("can't step past the first file", async () => {
    renderView(FILES[0]!);

    expect(await screen.findByText("patch of a.txt")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Previous file" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Next file" })).toBeEnabled();
  });

  it("doesn't load a binary file's patch", async () => {
    renderView(file("image.png", null, null));

    expect(await screen.findByText("Binary file")).toBeInTheDocument();
    expect(rpc.git.diff.commitFilePatch).not.toHaveBeenCalled();
  });

  it("says a file was only renamed", async () => {
    renderView({ ...file("new.txt", 0, 0), status: "renamed", origPath: "old.txt" });

    expect(
      await screen.findByText("Renamed from old.txt, with the same contents."),
    ).toBeInTheDocument();
    expect(rpc.git.diff.commitFilePatch).not.toHaveBeenCalled();
  });

  it("asks before loading a large change", async () => {
    renderView(file("big.lock", 30_000, 0));

    await userEvent.click(await screen.findByRole("button", { name: "Show anyway" }));
    expect(await screen.findByText("patch of big.lock")).toBeInTheDocument();
  });

  it("tries the next file afresh after one couldn't be shown", async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const [shown, setShown] = createSignal<ChangedFile>(file("broken.txt"));
    render(() => (
      <QueryClientProvider client={client}>
        <FileDiffView
          repositoryId="repo"
          source={COMMIT}
          file={shown()}
          onOpen={(_, next) => setShown(next)}
          onClose={() => {}}
        />
      </QueryClientProvider>
    ));

    expect(await screen.findByText("The patch has no file in it.")).toBeInTheDocument();
    setShown(FILES[1]!);
    expect(await screen.findByText("patch of b.txt")).toBeInTheDocument();
  });

  it("says an empty file was added", async () => {
    renderView({ ...file(".gitkeep", 0, 0), status: "added" });

    expect(await screen.findByText("An empty file was added.")).toBeInTheDocument();
  });

  it("leaves Esc to a popover that closes on it", async () => {
    const onClose = vi.fn();
    renderView(FILES[0]!, { onClose });
    await screen.findByText("patch of a.txt");
    document.addEventListener("keydown", popover);
    try {
      await userEvent.keyboard("{Escape}");
    } finally {
      document.removeEventListener("keydown", popover);
    }
    expect(onClose).not.toHaveBeenCalled();
  });

  it("gives the focus back to what opened it once it closes", async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const [open, setOpen] = createSignal(false);
    render(() => (
      <QueryClientProvider client={client}>
        <button type="button" onClick={() => setOpen(true)}>
          Open a.txt
        </button>
        <Show when={open()}>
          <FileDiffView
            repositoryId="repo"
            source={COMMIT}
            file={FILES[0]!}
            onOpen={() => {}}
            onClose={() => setOpen(false)}
          />
        </Show>
      </QueryClientProvider>
    ));

    await userEvent.click(screen.getByRole("button", { name: "Open a.txt" }));
    await userEvent.click(await screen.findByRole("button", { name: "Back to the history (Esc)" }));
    expect(screen.getByRole("button", { name: "Open a.txt" })).toHaveFocus();
  });

  it("goes back to the history with Esc", async () => {
    const onClose = vi.fn();
    renderView(FILES[0]!, { onClose });

    await screen.findByText("patch of a.txt");
    await userEvent.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledOnce();
  });
});

/** A patch of `path` with one line changed to `line`, as git would give it. */
function changedLine(path: string, line: string) {
  return `diff --git a/${path} b/${path}\nindex 1..2 100644\n--- a/${path}\n+++ b/${path}\n@@ -1 +1 @@\n-old\n+${line}\n`;
}

describe("an uncommitted file's changes", () => {
  beforeEach(() => {
    rpc.git.diff.unstagedFilePatch.mockReset();
    rpc.git.diff.unstagedFilePatch.mockImplementation(async ({ path }) =>
      changedLine(path, `new ${path}`),
    );
  });

  it("are updated in place when they change on disk", async () => {
    setChanges({ unstaged: [file("a.txt")] });
    const client = renderView(file("a.txt"), {}, UNSTAGED);
    expect(await screen.findByText(/\+new a\.txt/)).toBeInTheDocument();

    rpc.git.diff.unstagedFilePatch.mockImplementation(async () => changedLine("a.txt", "saved"));
    await client.invalidateQueries({ queryKey: gitKeys.uncommitted("repo") });
    expect(await screen.findByText(/\+saved/)).toBeInTheDocument();
  });

  it("says when the file has no unstaged changes left, with a way to its staged ones", async () => {
    setChanges({ unstaged: [file("a.txt")] });
    const onOpen = vi.fn();
    const client = renderView(file("a.txt"), { onOpen }, UNSTAGED);
    expect(await screen.findByText(/\+new a\.txt/)).toBeInTheDocument();

    setChanges({ staged: [file("a.txt")] });
    rpc.git.diff.unstagedFilePatch.mockImplementation(async () => "");
    await client.invalidateQueries({ queryKey: gitKeys.uncommitted("repo") });
    expect(await screen.findByText("No unstaged changes")).toBeInTheDocument();
    expect(screen.queryByText(/\+new a\.txt/)).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Show staged changes" }));
    expect(onOpen).toHaveBeenCalledWith({ kind: "staged" }, file("a.txt"));
  });

  it("tells from the patch whether an untracked file is binary", async () => {
    const image = { ...file("image.png", null, null), status: "untracked" } as const;
    setChanges({ unstaged: [image] });
    rpc.git.diff.unstagedFilePatch.mockImplementation(
      async () =>
        "diff --git a/image.png b/image.png\nnew file mode 100644\nindex 0000000..1111111\nBinary files /dev/null and b/image.png differ\n",
    );
    renderView(image, {}, UNSTAGED);

    expect(await screen.findByText("Binary file")).toBeInTheDocument();
    expect(rpc.git.diff.unstagedFilePatch).toHaveBeenCalledWith(
      { repositoryId: "repo", path: "image.png", origPath: null, untracked: true },
      expect.anything(),
    );
  });

  it("asks before showing a large file that wasn't counted", async () => {
    const log = file("huge.log", null, null);
    setChanges({ unstaged: [log], uncounted: true });
    const lines = "+line\n".repeat(25_000);
    rpc.git.diff.unstagedFilePatch.mockImplementation(
      async () =>
        `diff --git a/huge.log b/huge.log\nindex 1..2 100644\n@@ -0,0 +1,25000 @@\n${lines}`,
    );
    renderView(log, {}, UNSTAGED);

    expect(await screen.findByText(/25,000 lines changed/)).toBeInTheDocument();
  });

  it("doesn't load a conflicted file's patch", async () => {
    const conflicted = { ...file("both.txt"), status: "conflicted" } as const;
    setChanges({ unstaged: [conflicted] });
    renderView(conflicted, {}, UNSTAGED);

    expect(await screen.findByText("This file has conflicts")).toBeInTheDocument();
    expect(rpc.git.diff.unstagedFilePatch).not.toHaveBeenCalled();
  });

  it("stays on the page while they're refetched, so the view keeps its scroll position", async () => {
    setChanges({ unstaged: [file("a.txt")] });
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    client.setQueryDefaults(gitKeys.all, { staleTime: Infinity });
    render(() => (
      <QueryClientProvider client={client}>
        <Suspense>
          <FileDiffView
            repositoryId="repo"
            source={{ kind: "unstaged" }}
            file={file("a.txt")}
            onOpen={() => {}}
            onClose={() => {}}
          />
        </Suspense>
      </QueryClientProvider>
    ));
    const shown = await screen.findByText(/\+new a\.txt/);
    // Suspending takes the view off the page and puts it back, scrolled to the top.
    const removed: Node[] = [];
    const observer = new MutationObserver((records) => {
      for (const record of records) removed.push(...record.removedNodes);
    });
    observer.observe(document.body, { childList: true, subtree: true });

    setChanges({ unstaged: [file("a.txt", 2, 1)] });
    rpc.git.diff.unstagedFilePatch.mockImplementation(async () => changedLine("a.txt", "saved"));
    await client.invalidateQueries({ queryKey: gitKeys.uncommitted("repo") });
    expect(await screen.findByText(/\+saved/)).toBe(shown);
    observer.disconnect();
    expect(removed.filter((node) => node.contains(shown))).toEqual([]);
  });

  it("doesn't step from where another file was, for one that isn't in its list", async () => {
    setChanges({
      unstaged: [file("a.txt"), file("b.txt"), file("c.txt")],
      staged: [file("d.txt")],
    });
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const [open, setOpen] = createSignal<{ source: DiffSource; file: ChangedFile }>({
      source: UNSTAGED,
      file: file("b.txt"),
    });
    render(() => (
      <QueryClientProvider client={client}>
        <FileDiffView
          repositoryId="repo"
          source={open().source}
          file={open().file}
          onOpen={() => {}}
          onClose={() => {}}
        />
      </QueryClientProvider>
    ));
    expect(await screen.findByText(/\+new b\.txt/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Next file" })).toBeEnabled();

    // Opened from the staged list as it was being unstaged.
    setOpen({ source: { kind: "staged" }, file: file("a.txt") });
    expect(await screen.findByText("No staged changes")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Previous file" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Next file" })).toBeDisabled();
  });

  it("doesn't load a file without line counts ahead, which can be of any size", async () => {
    const untracked = (path: string) =>
      ({ ...file(path, null, null), status: "untracked" }) as const;
    setChanges({ unstaged: [file("a.txt"), untracked("b.log"), untracked("c.log")] });
    renderView(untracked("b.log"), {}, UNSTAGED);

    expect(await screen.findByText(/\+new b\.log/)).toBeInTheDocument();
    await vi.waitFor(() =>
      expect(rpc.git.diff.unstagedFilePatch.mock.calls.map(([input]) => input.path)).toContain(
        "a.txt",
      ),
    );
    expect(rpc.git.diff.unstagedFilePatch.mock.calls.map(([input]) => input.path)).not.toContain(
      "c.log",
    );
  });
});
