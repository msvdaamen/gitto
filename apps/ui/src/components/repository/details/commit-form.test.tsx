import { render, screen } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { createSignal, Suspense } from "solid-js";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { LastCommit } from "@/git/status";

import { CommitForm } from "./commit-form";

const rpc = vi.hoisted(() => ({
  git: {
    commit: {
      create: vi.fn(async () => {}),
      message: vi.fn(async ({ sha }: { sha: string }) => `Commit ${sha}\n\nIts body`),
    },
  },
}));

vi.mock("@/lib/rpc", () => ({ rpc }));

function renderForm(props: { stagedCount?: number; lastCommit?: LastCommit | null }) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const [lastCommit, setLastCommit] = createSignal<LastCommit | undefined>(
    props.lastCommit === null
      ? undefined
      : (props.lastCommit ?? { sha: "a1", pushedTo: undefined }),
  );
  render(() => (
    <QueryClientProvider client={client}>
      <Suspense fallback="Suspended">
        <CommitForm
          repositoryId="repo"
          stagedCount={props.stagedCount ?? 0}
          lastCommit={lastCommit()}
        />
      </Suspense>
    </QueryClientProvider>
  ));
  return { setLastCommit };
}

describe("amending the last commit", () => {
  afterEach(() => {
    vi.clearAllMocks();
  });

  it("edits the last commit's message, and amends it without anything staged", async () => {
    renderForm({});
    const summary = screen.getByLabelText("Commit message");
    expect(screen.getByRole("button", { name: /Nothing staged/ })).toBeDisabled();

    await userEvent.click(screen.getByLabelText("Amend last commit"));
    expect(await screen.findByDisplayValue("Commit a1")).toBe(summary);
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
  });

  it("keeps the message being written, whether amending is turned off or done", async () => {
    renderForm({ stagedCount: 2 });
    const summary = screen.getByLabelText("Commit message");
    await userEvent.type(summary, "Draft");

    const amend = screen.getByLabelText("Amend last commit");
    await userEvent.click(amend);
    expect(await screen.findByDisplayValue("Commit a1")).toBe(summary);
    expect(screen.getByRole("button", { name: /Amend with 2 files/ })).toBeEnabled();
    await userEvent.click(amend);
    expect(summary).toHaveValue("Draft");

    await userEvent.click(amend);
    await userEvent.click(await screen.findByRole("button", { name: /Amend with 2 files/ }));
    expect(await screen.findByDisplayValue("Draft")).toBe(summary);
  });

  it("loads the new last commit's message when HEAD moves", async () => {
    const { setLastCommit } = renderForm({});
    await userEvent.click(screen.getByLabelText("Amend last commit"));
    const summary = await screen.findByDisplayValue("Commit a1");
    await userEvent.type(summary, " edited");

    setLastCommit({ sha: "b2", pushedTo: undefined });
    expect(await screen.findByDisplayValue("Commit b2")).toBe(summary);
  });

  it("warns when the last commit is already pushed", async () => {
    renderForm({ lastCommit: { sha: "a1", pushedTo: "origin/main" } });
    await userEvent.click(screen.getByLabelText("Amend last commit"));
    expect(await screen.findByText(/already on origin\/main/)).toBeInTheDocument();
  });

  it("can't amend before the first commit", () => {
    renderForm({ lastCommit: null });
    expect(screen.getByLabelText("Amend last commit")).toBeDisabled();
  });
});
