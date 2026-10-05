import type { CommitRef } from "@gitto/git/types";
import { render, screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { toRefLabels } from "@/git/ref-labels";

import { HistoryRefLabels } from "./ref-labels";

/** A commit's labels, with what double-clicking them switches to. */
function renderLabels(refs: CommitRef[]) {
  const onSwitch = vi.fn();
  render(() => (
    <HistoryRefLabels labels={toRefLabels(refs)} search="" color="red" onSwitch={onSwitch} />
  ));
  return onSwitch;
}

describe("switching branches from the history", () => {
  it("switches to a local branch that's double-clicked", async () => {
    const user = userEvent.setup();
    const onSwitch = renderLabels([
      { kind: "local", name: "feature", fullName: "refs/heads/feature" },
      { kind: "remote", name: "origin/feature", fullName: "refs/remotes/origin/feature" },
    ]);

    await user.click(screen.getByText("feature"));
    expect(onSwitch).not.toHaveBeenCalled();
    await user.dblClick(screen.getByText("feature"));
    expect(onSwitch).toHaveBeenCalledWith("refs/heads/feature");
  });

  it("switches to a remote branch without a local copy here", async () => {
    const user = userEvent.setup();
    const onSwitch = renderLabels([
      { kind: "remote", name: "origin/feature", fullName: "refs/remotes/origin/feature" },
    ]);

    await user.dblClick(screen.getByText("origin/feature"));
    expect(onSwitch).toHaveBeenCalledWith("refs/remotes/origin/feature");
  });

  it("switches to a branch only shown when the count of the rest is hovered", async () => {
    const user = userEvent.setup();
    const onSwitch = renderLabels([
      { kind: "local", name: "main", fullName: "refs/heads/main", current: true },
      { kind: "remote", name: "origin/release", fullName: "refs/remotes/origin/release" },
    ]);
    expect(screen.queryByText("origin/release")).not.toBeInTheDocument();

    await user.hover(screen.getByText("+1"));
    await user.dblClick(await screen.findByText("origin/release"));
    expect(onSwitch).toHaveBeenCalledWith("refs/remotes/origin/release");
  });

  it("doesn't switch to a tag or a detached HEAD", async () => {
    const user = userEvent.setup();
    const onSwitchTag = renderLabels([{ kind: "tag", name: "v1", fullName: "refs/tags/v1" }]);
    await user.dblClick(screen.getByText("v1"));
    const onSwitchHead = renderLabels([{ kind: "head", name: "HEAD", fullName: "HEAD" }]);
    await user.dblClick(screen.getByText("HEAD"));

    expect(onSwitchTag).not.toHaveBeenCalled();
    expect(onSwitchHead).not.toHaveBeenCalled();
  });
});
