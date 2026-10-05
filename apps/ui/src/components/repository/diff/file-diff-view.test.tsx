import type { ChangedFile } from "@gitto/git/types";
import { render, screen } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { createEffect, createSignal, on, Show } from "solid-js";
import { afterEach, describe, expect, it, vi } from "vitest";

import { FileDiffView } from "./file-diff-view";

const rpc = vi.hoisted(() => ({
  git: {
    diff: {
      commitFiles: async () => FILES,
      commitFilePatch: vi.fn(patchOf),
    },
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

/** Closes on Esc like a popover's layer: on the document, marking it handled. */
const popover = (event: KeyboardEvent) => event.preventDefault();

function renderView(
  shown: ChangedFile,
  handlers: Partial<{ onOpen: () => void; onClose: () => void }> = {},
) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(() => (
    <QueryClientProvider client={client}>
      <FileDiffView
        repositoryId="repo"
        sha="a1"
        file={shown}
        onOpen={handlers.onOpen ?? (() => {})}
        onClose={handlers.onClose ?? (() => {})}
      />
    </QueryClientProvider>
  ));
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
    expect(onOpen).toHaveBeenLastCalledWith(FILES[2]);
    await userEvent.click(screen.getByRole("button", { name: "Previous file" }));
    expect(onOpen).toHaveBeenLastCalledWith(FILES[0]);
  });

  it("keeps the last file on show, header and all, until the next one's changes are in", async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const [shown, setShown] = createSignal(FILES[0]!);
    render(() => (
      <QueryClientProvider client={client}>
        <FileDiffView
          repositoryId="repo"
          sha="a1"
          file={shown()}
          onOpen={setShown}
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
          sha="a1"
          file={shown()}
          onOpen={setShown}
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
            sha="a1"
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
