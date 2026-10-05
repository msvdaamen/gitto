import type { ChangedFile, Uncommitted, WorkingTreeFiles } from "@gitto/git/types";
import { render, screen } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { createEffect, createSignal, on, Show, Suspense } from "solid-js";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { DiffSource } from "@/git/diff-source";
import { gitKeys } from "@/git/queries/keys";

import { FileDiffView } from "./file-diff-view";
import type { LineStaging } from "./patch-viewer";

const rpc = vi.hoisted(() => ({
  git: {
    diff: {
      commitFiles: async () => FILES,
      commitFilePatch: vi.fn(patchOf),
      unstagedFilePatch: vi.fn(patchOf),
      stagedFilePatch: vi.fn(patchOf),
    },
    status: { get: vi.fn(async () => uncommitted) },
    staging: {
      stage: vi.fn<(input: unknown) => Promise<void>>(),
      unstage: vi.fn<(input: unknown) => Promise<void>>(),
      stageLines: vi.fn<(input: unknown) => Promise<{ patch: string }>>(),
      unstageLines: vi.fn<(input: unknown) => Promise<{ patch: string }>>(),
    },
  },
}));

/** The props the viewer on show was given, to pick lines to stage with. */
const viewer = vi.hoisted(() => ({ props: undefined as ViewerProps | undefined }));

type ViewerProps = { patch: string; onShown: () => void; staging?: LineStaging };

vi.mock("@/lib/rpc", () => ({ rpc }));
// The viewer needs a layout and workers, which jsdom doesn't have.
vi.mock("./patch-viewer", () => ({
  default: (props: ViewerProps) => {
    viewer.props = props;
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

/** The staging the viewer on show was given, once there is some. */
function staging(): LineStaging {
  expect(viewer.props?.staging).toBeDefined();
  return viewer.props!.staging!;
}

describe("staging lines", () => {
  const LINES = { deletions: [{ start: 1, end: 1 }], additions: [{ start: 1, end: 1 }] };
  beforeEach(() => {
    viewer.props = undefined;
    rpc.git.diff.unstagedFilePatch.mockReset();
    rpc.git.diff.unstagedFilePatch.mockImplementation(async ({ path }) =>
      changedLine(path, `new ${path}`),
    );
    rpc.git.diff.stagedFilePatch.mockReset();
    rpc.git.diff.stagedFilePatch.mockImplementation(async ({ path }) =>
      changedLine(path, `staged ${path}`),
    );
    rpc.git.staging.stageLines.mockReset();
    rpc.git.staging.unstageLines.mockReset();
  });

  it("stages the lines picked from the patch on show, and shows the patch they leave", async () => {
    setChanges({ unstaged: [file("a.txt"), file("b.txt")] });
    renderView(file("a.txt"), {}, UNSTAGED);
    expect(await screen.findByText(/\+new a\.txt/)).toBeInTheDocument();
    rpc.git.staging.stageLines.mockResolvedValue({ patch: changedLine("a.txt", "left") });
    const fetchesOfA = () =>
      rpc.git.diff.unstagedFilePatch.mock.calls.filter(([{ path }]) => path === "a.txt").length;
    const fetched = fetchesOfA();
    const statuses = rpc.git.status.get.mock.calls.length;

    expect(staging().action).toBe("stage");
    await staging().onStage(changedLine("a.txt", "new a.txt"), LINES);
    expect(rpc.git.staging.stageLines).toHaveBeenCalledWith({
      repositoryId: "repo",
      path: "a.txt",
      origPath: null,
      untracked: false,
      patch: changedLine("a.txt", "new a.txt"),
      lines: LINES,
    });
    expect(await screen.findByText(/\+left/)).toBeInTheDocument();
    // The lists are refetched, but not the patch, which came back with the lines.
    await vi.waitFor(() => expect(rpc.git.status.get.mock.calls.length).toBeGreaterThan(statuses));
    expect(fetchesOfA()).toBe(fetched);
  });

  it("stages lines of an untracked file as such", async () => {
    const notes = { ...file("notes.txt", null, null), status: "untracked" } as const;
    setChanges({ unstaged: [notes] });
    renderView(notes, {}, UNSTAGED);
    await screen.findByText(/\+new notes\.txt/);
    rpc.git.staging.stageLines.mockResolvedValue({ patch: changedLine("notes.txt", "rest") });

    await staging().onStage("the patch", LINES);
    expect(rpc.git.staging.stageLines).toHaveBeenCalledWith(
      expect.objectContaining({ path: "notes.txt", untracked: true }),
    );
  });

  it("unstages lines picked from the staged changes", async () => {
    setChanges({ staged: [file("a.txt")] });
    renderView(file("a.txt"), {}, { kind: "staged" });
    expect(await screen.findByText(/\+staged a\.txt/)).toBeInTheDocument();
    rpc.git.staging.unstageLines.mockResolvedValue({ patch: changedLine("a.txt", "still staged") });

    expect(staging().action).toBe("unstage");
    await staging().onStage("the patch", LINES);
    expect(rpc.git.staging.unstageLines).toHaveBeenCalledWith({
      repositoryId: "repo",
      path: "a.txt",
      origPath: null,
      patch: "the patch",
      lines: LINES,
    });
    expect(await screen.findByText(/\+still staged/)).toBeInTheDocument();
  });

  it("is busy until the patch the lines leave is on show", async () => {
    setChanges({ unstaged: [file("a.txt")] });
    renderView(file("a.txt"), {}, UNSTAGED);
    await screen.findByText(/\+new a\.txt/);
    let done!: (result: { patch: string }) => void;
    rpc.git.staging.stageLines.mockReturnValue(new Promise((resolve) => (done = resolve)));

    expect(staging().busy).toBe(false);
    const staged = staging().onStage(changedLine("a.txt", "new a.txt"), LINES);
    await vi.waitFor(() => expect(staging().busy).toBe(true));
    done({ patch: changedLine("a.txt", "left") });
    await staged;
    await screen.findByText(/\+left/);
    expect(staging().busy).toBe(false);
  });

  it("opens the next file once the last lines are staged, this one's staying on show till then", async () => {
    setChanges({ unstaged: [file("a.txt"), file("b.txt")] });
    const onOpen = vi.fn();
    renderView(file("a.txt"), { onOpen }, UNSTAGED);
    await screen.findByText(/\+new a\.txt/);
    rpc.git.staging.stageLines.mockResolvedValue({ patch: "" });

    await staging().onStage(changedLine("a.txt", "new a.txt"), LINES);
    expect(onOpen).toHaveBeenCalledWith(UNSTAGED, file("b.txt"));
    expect(screen.queryByText("No unstaged changes")).not.toBeInTheDocument();
    expect(screen.getByText(/\+new a\.txt/)).toBeInTheDocument();
  });

  it("opens the one before, once the last file's last lines are staged", async () => {
    setChanges({ unstaged: [file("a.txt"), file("b.txt")] });
    const onOpen = vi.fn();
    renderView(file("b.txt"), { onOpen }, UNSTAGED);
    await screen.findByText(/\+new b\.txt/);
    // Only its mode change is left.
    rpc.git.staging.stageLines.mockResolvedValue({
      patch: "diff --git a/b.txt b/b.txt\nold mode 100644\nnew mode 100755\n",
    });

    await staging().onStage(changedLine("b.txt", "new b.txt"), LINES);
    expect(onOpen).toHaveBeenCalledWith(UNSTAGED, file("a.txt"));
  });

  it("says why lines couldn't be staged, and shows the changes as they are now", async () => {
    setChanges({ unstaged: [file("a.txt")] });
    renderView(file("a.txt"), {}, UNSTAGED);
    await screen.findByText(/\+new a\.txt/);
    const message = "The file changed since its changes were shown, so nothing was staged.";
    rpc.git.staging.stageLines.mockRejectedValue(new Error(message));
    rpc.git.diff.unstagedFilePatch.mockImplementation(async () => changedLine("a.txt", "saved"));

    await expect(staging().onStage(changedLine("a.txt", "new a.txt"), LINES)).rejects.toThrow(
      message,
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(message);
    expect(await screen.findByText(/\+saved/)).toBeInTheDocument();
  });

  it("doesn't pick lines of a commit's changes, nor of a link's", async () => {
    renderView(FILES[0]!);
    await screen.findByText("patch of a.txt");
    expect(viewer.props?.staging).toBeUndefined();

    viewer.props = undefined;
    setChanges({ unstaged: [file("link")] });
    rpc.git.diff.unstagedFilePatch.mockImplementation(
      async () =>
        "diff --git a/link b/link\nindex 1..2 120000\n--- a/link\n+++ b/link\n@@ -1 +1 @@\n-a\n\\ No newline at end of file\n+b\n\\ No newline at end of file\n",
    );
    renderView(file("link"), {}, UNSTAGED);
    await screen.findByText(/\+b/);
    const shown = viewer.props as ViewerProps | undefined;
    expect(shown).toBeDefined();
    expect(shown?.staging).toBeUndefined();
  });
});

describe("staging the whole file", () => {
  beforeEach(() => {
    viewer.props = undefined;
    rpc.git.diff.unstagedFilePatch.mockReset();
    rpc.git.diff.unstagedFilePatch.mockImplementation(async ({ path }) =>
      changedLine(path, `new ${path}`),
    );
    rpc.git.diff.stagedFilePatch.mockReset();
    rpc.git.diff.stagedFilePatch.mockImplementation(async ({ path }) =>
      changedLine(path, `staged ${path}`),
    );
    rpc.git.staging.stage.mockReset();
    rpc.git.staging.unstage.mockReset();
  });

  it("stages it, then opens the next file in the list", async () => {
    setChanges({ unstaged: [file("a.txt"), file("b.txt")] });
    const onOpen = vi.fn();
    renderView(file("a.txt"), { onOpen }, UNSTAGED);
    await screen.findByText(/\+new a\.txt/);
    rpc.git.staging.stage.mockImplementation(async () =>
      setChanges({ staged: [file("a.txt")], unstaged: [file("b.txt")] }),
    );

    await userEvent.click(screen.getByRole("button", { name: "Stage file" }));
    expect(rpc.git.staging.stage).toHaveBeenCalledWith({ repositoryId: "repo", paths: ["a.txt"] });
    await vi.waitFor(() => expect(onOpen).toHaveBeenCalledWith(UNSTAGED, file("b.txt")));
  });

  it("stages a renamed file by both its paths", async () => {
    const renamed = { ...file("new.txt"), status: "renamed", origPath: "old.txt" } as const;
    setChanges({ unstaged: [renamed] });
    renderView(renamed, {}, UNSTAGED);
    await screen.findByText(/\+new new\.txt/);
    rpc.git.staging.stage.mockResolvedValue(undefined);

    await userEvent.click(screen.getByRole("button", { name: "Stage file" }));
    expect(rpc.git.staging.stage).toHaveBeenCalledWith({
      repositoryId: "repo",
      paths: ["new.txt", "old.txt"],
    });
  });

  it("unstages a staged file, then opens the one before the last", async () => {
    setChanges({ staged: [file("a.txt"), file("b.txt")] });
    const onOpen = vi.fn();
    renderView(file("b.txt"), { onOpen }, { kind: "staged" });
    await screen.findByText(/\+staged b\.txt/);
    rpc.git.staging.unstage.mockImplementation(async () =>
      setChanges({ staged: [file("a.txt")], unstaged: [file("b.txt")] }),
    );

    await userEvent.click(screen.getByRole("button", { name: "Unstage file" }));
    expect(rpc.git.staging.unstage).toHaveBeenCalledWith({
      repositoryId: "repo",
      paths: ["b.txt"],
    });
    await vi.waitFor(() => expect(onOpen).toHaveBeenCalledWith({ kind: "staged" }, file("a.txt")));
  });

  it("says why it couldn't be staged", async () => {
    setChanges({ unstaged: [file("a.txt")] });
    const onOpen = vi.fn();
    renderView(file("a.txt"), { onOpen }, UNSTAGED);
    await screen.findByText(/\+new a\.txt/);
    rpc.git.staging.stage.mockRejectedValue(new Error("The index is locked."));

    await userEvent.click(screen.getByRole("button", { name: "Stage file" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("The index is locked.");
    expect(onOpen).not.toHaveBeenCalled();
  });

  it("opens the next file once it's staged, without waiting for the lists", async () => {
    setChanges({ unstaged: [file("a.txt"), file("b.txt")] });
    const onOpen = vi.fn();
    renderView(file("a.txt"), { onOpen }, UNSTAGED);
    await screen.findByText(/\+new a\.txt/);
    rpc.git.staging.stage.mockResolvedValue(undefined);
    rpc.git.status.get.mockImplementation(() => new Promise(() => {}));

    try {
      await userEvent.click(screen.getByRole("button", { name: "Stage file" }));
      await vi.waitFor(() => expect(onOpen).toHaveBeenCalledWith(UNSTAGED, file("b.txt")));
    } finally {
      rpc.git.status.get.mockImplementation(async () => uncommitted);
    }
  });

  it("stages nothing else while it's staged: not again, nor its lines", async () => {
    setChanges({ unstaged: [file("a.txt")] });
    renderView(file("a.txt"), {}, UNSTAGED);
    await screen.findByText(/\+new a\.txt/);
    let done!: () => void;
    rpc.git.staging.stage.mockReturnValue(new Promise<void>((resolve) => (done = resolve)));

    const button = screen.getByRole("button", { name: "Stage file" });
    await userEvent.click(button);
    expect(button).toBeDisabled();
    expect(staging().busy).toBe(true);
    await userEvent.click(button);
    expect(rpc.git.staging.stage).toHaveBeenCalledTimes(1);
    done();
    await vi.waitFor(() => expect(staging().busy).toBe(false));
  });

  it("can't be staged while the last file is still on show, as another's changes load", async () => {
    setChanges({ unstaged: [file("a.txt"), file("b.txt")] });
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const [shown, setShown] = createSignal<ChangedFile>(file("a.txt"));
    render(() => (
      <QueryClientProvider client={client}>
        <FileDiffView
          repositoryId="repo"
          source={UNSTAGED}
          file={shown()}
          onOpen={(_, next) => setShown(next)}
          onClose={() => {}}
        />
      </QueryClientProvider>
    ));
    rpc.git.diff.unstagedFilePatch.mockImplementation(async ({ path }) =>
      path === "b.txt" ? new Promise<string>(() => {}) : changedLine(path, `new ${path}`),
    );
    await screen.findByText(/\+new a\.txt/);

    await userEvent.click(screen.getByRole("button", { name: "Next file" }));
    expect(screen.getByRole("region", { name: "Changes in a.txt" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Stage file" })).toBeDisabled();
  });

  it("isn't offered for a repository inside this one", async () => {
    const nested = { ...file("sub/", null, null), status: "untracked" } as const;
    setChanges({ unstaged: [nested] });
    renderView(nested, {}, UNSTAGED);
    expect(await screen.findByText("Another repository")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Stage file" })).not.toBeInTheDocument();
  });

  it("isn't offered for a commit's file", async () => {
    renderView(FILES[0]!);
    await screen.findByText("patch of a.txt");
    expect(screen.queryByRole("button", { name: /stage file/i })).not.toBeInTheDocument();
  });
});
