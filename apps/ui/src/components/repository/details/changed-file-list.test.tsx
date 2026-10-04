import { render, screen } from "@solidjs/testing-library";
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
