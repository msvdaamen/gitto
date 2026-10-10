import type { VersionChanges } from "@gitto/system/types";
import { render, screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { whatsNew } from "@/hooks/whats-new";

import { WhatsNewDialog } from "./whats-new-dialog";

const rpc = vi.hoisted(() => ({
  system: { changelog: { all: vi.fn(), unseen: vi.fn(), seen: vi.fn() } },
}));

vi.mock("@/lib/rpc", () => ({ rpc }));

const versions: VersionChanges[] = [
  {
    version: "1.2.4-nightly29860000",
    date: "2026-10-12",
    changes: [
      { type: "fix", scope: "diff", breaking: false, description: "keep the scroll", commit: "b" },
      {
        type: "feat",
        breaking: false,
        description: "add worktrees",
        detail: "Open one by double-clicking it.",
        pr: 74,
        commit: "a",
      },
    ],
  },
  {
    version: "1.2.3",
    date: "2026-10-10",
    changes: [{ type: "perf", breaking: true, description: "load faster", commit: "c" }],
  },
];

beforeEach(() => {
  rpc.system.changelog.seen.mockResolvedValue(undefined);
});

afterEach(() => {
  whatsNew.close();
  vi.resetAllMocks();
});

describe("what's new", () => {
  it("shows what changed since the last version seen, and remembers it seen once closed", async () => {
    const user = userEvent.setup();
    rpc.system.changelog.unseen.mockResolvedValue(versions);
    render(() => <WhatsNewDialog />);

    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent("Gitto has updated.");
    expect(screen.getByRole("heading", { name: /^Nightly/ })).toHaveTextContent(
      "v1.2.4-nightly29860000",
    );
    expect(screen.getByRole("heading", { name: /^Gitto 1\.2\.3/ })).toBeInTheDocument();
    const items = screen.getAllByRole("listitem").map((item) => item.textContent);
    expect(items).toEqual([
      "NewAdd worktrees#74Open one by double-clicking it.",
      "FixedDiff: Keep the scroll",
      "BreakingLoad faster",
    ]);

    await user.click(screen.getByRole("button", { name: "Got it" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(rpc.system.changelog.seen).toHaveBeenCalled();
  });

  it("doesn't open with nothing new", async () => {
    rpc.system.changelog.unseen.mockResolvedValue([]);
    render(() => <WhatsNewDialog />);

    await vi.waitFor(() => expect(rpc.system.changelog.unseen).toHaveBeenCalled());
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("shows every version when asked", async () => {
    rpc.system.changelog.unseen.mockResolvedValue([]);
    rpc.system.changelog.all.mockResolvedValue(versions);
    render(() => <WhatsNewDialog />);

    await whatsNew.showAll();
    expect(await screen.findByRole("dialog")).toHaveTextContent(
      "What changed in each version, the newest first.",
    );
    expect(screen.getAllByRole("heading", { level: 3 })).toHaveLength(2);
  });
});
