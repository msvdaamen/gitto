import { render, screen } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { CommitForm } from "./commit-form";

const rpc = vi.hoisted(() => ({
  git: {
    history: {
      commit: vi.fn(async ({ sha }: { sha: string }) => ({
        sha,
        parents: [],
        authorName: "Ada Lovelace",
        authorEmail: "ada@example.com",
        authoredAt: 0,
        committedAt: 0,
        refs: [],
        subject: "Last commit",
        body: "Its body",
      })),
    },
    commit: { create: vi.fn(async () => {}) },
  },
}));

vi.mock("@/lib/rpc", () => ({ rpc }));

function renderForm(props: { stagedCount?: number; pushedTo?: string; lastCommit?: string }) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(() => (
    <QueryClientProvider client={client}>
      <CommitForm
        repositoryId="repo"
        stagedCount={props.stagedCount ?? 0}
        lastCommit={"lastCommit" in props ? props.lastCommit : "abc"}
        pushedTo={props.pushedTo}
      />
    </QueryClientProvider>
  ));
}

describe("amending the last commit", () => {
  afterEach(() => {
    vi.clearAllMocks();
  });

  it("edits the last commit's message, and commits with --amend without anything staged", async () => {
    renderForm({});
    const summary = screen.getByLabelText("Commit message");
    expect(screen.getByRole("button", { name: /Nothing staged/ })).toBeDisabled();

    await userEvent.click(screen.getByLabelText("Amend last commit"));
    expect(await screen.findByDisplayValue("Last commit")).toBe(summary);
    expect(screen.getByLabelText(/Description/)).toHaveValue("Its body");

    await userEvent.clear(summary);
    await userEvent.type(summary, "Reworded");
    await userEvent.click(screen.getByRole("button", { name: /Amend message/ }));
    expect(rpc.git.commit.create).toHaveBeenCalledWith({
      repositoryId: "repo",
      message: "Reworded\n\nIts body",
      amend: true,
    });
    expect(await screen.findByLabelText("Amend last commit")).not.toBeChecked();
    expect(summary).toHaveValue("");
  });

  it("puts back the message being written when amending is turned off", async () => {
    renderForm({ stagedCount: 2 });
    const summary = screen.getByLabelText("Commit message");
    await userEvent.type(summary, "Draft");

    const amend = screen.getByLabelText("Amend last commit");
    await userEvent.click(amend);
    expect(await screen.findByDisplayValue("Last commit")).toBe(summary);
    expect(screen.getByRole("button", { name: /Amend with 2 files/ })).toBeEnabled();

    await userEvent.click(amend);
    expect(summary).toHaveValue("Draft");
    await userEvent.click(screen.getByRole("button", { name: /Commit 2 files/ }));
    expect(rpc.git.commit.create).toHaveBeenCalledWith({
      repositoryId: "repo",
      message: "Draft",
      amend: false,
    });
  });

  it("warns when the last commit is already pushed", async () => {
    renderForm({ pushedTo: "origin/main" });
    await userEvent.click(screen.getByLabelText("Amend last commit"));
    expect(await screen.findByText(/already on origin\/main/)).toBeInTheDocument();
  });

  it("can't amend before the first commit", () => {
    renderForm({ lastCommit: undefined });
    expect(screen.getByLabelText("Amend last commit")).toBeDisabled();
  });
});
