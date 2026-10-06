import { render, screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ChangedFileList } from "./changed-file-list";

const FILES = [
  { path: "image.png", status: "modified", origPath: null, additions: null, deletions: null },
  { path: "new.txt", status: "untracked", origPath: null, additions: null, deletions: null },
] as const;

describe("a file without line counts", () => {
  beforeEach(() => {
    // jsdom has no layout: give the list some room, so its rows render.
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(500);
    vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(500);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("is binary, or new when it's untracked", async () => {
    const scroller = document.createElement("div");
    render(() => <ChangedFileList files={[...FILES]} scrollElement={scroller} />);

    expect(await screen.findByText("image.png")).toBeInTheDocument();
    expect(screen.getByText("binary")).toBeInTheDocument();
    expect(screen.getByText("new")).toBeInTheDocument();
  });

  it("isn't taken for binary when no file's lines were counted", async () => {
    const scroller = document.createElement("div");
    render(() => <ChangedFileList files={[...FILES]} uncounted scrollElement={scroller} />);

    expect(await screen.findByText("image.png")).toBeInTheDocument();
    expect(screen.queryByText("binary")).not.toBeInTheDocument();
    expect(screen.getByText("new")).toBeInTheDocument();
  });
});

describe("opening a file", () => {
  beforeEach(() => {
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(500);
    vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(500);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("loads a file's changes ahead once the pointer rests on it", async () => {
    const onPrefetch = vi.fn();
    const scroller = document.createElement("div");
    render(() => (
      <ChangedFileList
        files={[...FILES]}
        scrollElement={scroller}
        onOpen={() => {}}
        onPrefetch={onPrefetch}
      />
    ));
    const row = await screen.findByRole("button", { name: "Show changes in image.png" });

    // Passed over on the way somewhere else: nothing's loaded.
    await userEvent.hover(row);
    await userEvent.unhover(row);
    await new Promise((resolve) => setTimeout(resolve, 120));
    expect(onPrefetch).not.toHaveBeenCalled();

    await userEvent.hover(row);
    await vi.waitFor(() => expect(onPrefetch).toHaveBeenCalledWith(FILES[0]));
  });

  it("shows its changes when it's clicked, and marks the one on show", async () => {
    const onOpen = vi.fn();
    const scroller = document.createElement("div");
    render(() => (
      <ChangedFileList
        files={[...FILES]}
        scrollElement={scroller}
        onOpen={onOpen}
        openPath="new.txt"
      />
    ));

    await userEvent.click(await screen.findByRole("button", { name: "Show changes in image.png" }));
    expect(onOpen).toHaveBeenCalledWith(FILES[0]);
    expect(screen.getByRole("button", { name: "Show changes in new.txt" })).toHaveAttribute(
      "aria-current",
      "true",
    );
    expect(screen.getByRole("button", { name: "Show changes in image.png" })).not.toHaveAttribute(
      "aria-current",
    );
  });
});

/** A conflicted file at `path`. */
function conflicted(path: string) {
  return { path, status: "conflicted", origPath: null, additions: 1, deletions: 1 } as const;
}

describe("a conflicted file", () => {
  beforeEach(() => {
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(500);
    vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(500);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("is badged as ready once it has no conflict markers left", async () => {
    const scroller = document.createElement("div");
    render(() => (
      <ChangedFileList
        files={[conflicted("a.txt"), conflicted("b.txt")]}
        markerFree={new Set(["b.txt"])}
        scrollElement={scroller}
      />
    ));

    expect(await screen.findByTitle("conflicted")).toHaveTextContent("!");
    expect(screen.getByTitle("conflicted, but resolved: ready to mark resolved")).toHaveTextContent(
      "✓",
    );
  });
});
