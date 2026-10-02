import { render, screen } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { createSignal, Suspense } from "solid-js";
import { afterEach, describe, expect, it, vi } from "vitest";

import { CommitForm } from "./commit-form";

const rpc = vi.hoisted(() => ({
  git: {
    commit: {
      create: vi.fn(async () => {}),
      message: vi.fn(async ({ sha }: { sha: string }) => `Commit ${sha}\n\nIts body`),
      pushedTo: vi.fn(async (): Promise<string | null> => null),
    },
  },
}));

vi.mock("@/lib/rpc", () => ({ rpc }));

function renderForm(props: { stagedCount?: number; lastCommit?: string | null } = {}) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const [lastCommit, setLastCommit] = createSignal<string | undefined>(
    props.lastCommit === null ? undefined : (props.lastCommit ?? "a1"),
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
  return {
    setLastCommit,
    summary: screen.getByLabelText("Commit message"),
    amend: screen.getByLabelText("Amend last commit"),
  };
}

describe("amending the last commit", () => {
  afterEach(() => {
    vi.clearAllMocks();
  });

  it("edits the last commit's message, and amends it without anything staged", async () => {
    const { summary, amend } = renderForm();
    expect(screen.getByRole("button", { name: /Nothing staged/ })).toBeDisabled();

    await userEvent.click(amend);
    expect(await screen.findByDisplayValue("Commit a1")).toBe(summary);
    expect(screen.getByLabelText(/Description/)).toHaveValue("Its body");

    await userEvent.clear(summary);
    await userEvent.type(summary, "Reworded");
    await userEvent.click(screen.getByRole("button", { name: /Amend message/ }));
    expect(rpc.git.commit.create).toHaveBeenCalledWith({
      repositoryId: "repo",
      message: "Reworded\n\nIts body",
      amend: "a1",
    });
    expect(amend).not.toBeChecked();
  });

  it("amends an untouched message exactly as it was written", async () => {
    rpc.git.commit.message.mockResolvedValueOnce("First line\nsecond line\n\n    code();");
    const { amend } = renderForm({ stagedCount: 1 });
    await userEvent.click(amend);
    await userEvent.click(await screen.findByRole("button", { name: /Amend with 1 file/ }));
    expect(rpc.git.commit.create).toHaveBeenCalledWith({
      repositoryId: "repo",
      message: "First line\nsecond line\n\n    code();",
      amend: "a1",
    });
  });

  it("can't be edited until the last commit's message has loaded", async () => {
    let resolve!: (message: string) => void;
    rpc.git.commit.message.mockReturnValueOnce(new Promise((r) => (resolve = r)));
    const { summary, amend } = renderForm();
    await userEvent.click(amend);
    expect(summary).toBeDisabled();

    resolve("Loaded");
    expect(await screen.findByDisplayValue("Loaded")).toBeEnabled();
  });

  it("keeps both messages when switching between them, and the new one after amending", async () => {
    const { summary, amend } = renderForm({ stagedCount: 2 });
    await userEvent.type(summary, "Draft");

    await userEvent.click(amend);
    await screen.findByDisplayValue("Commit a1");
    await userEvent.type(summary, " edited");
    await userEvent.click(amend);
    expect(summary).toHaveValue("Draft");
    await userEvent.click(amend);
    expect(summary).toHaveValue("Commit a1 edited");

    await userEvent.click(screen.getByRole("button", { name: /Amend with 2 files/ }));
    expect(await screen.findByDisplayValue("Draft")).toBe(summary);
  });

  it("stops amending once HEAD moves on, without loading the new commit's message", async () => {
    const { setLastCommit, summary, amend } = renderForm({ stagedCount: 1 });
    await userEvent.type(summary, "Draft");
    await userEvent.click(amend);
    await screen.findByDisplayValue("Commit a1");

    // Made in a terminal, say.
    setLastCommit("b2");
    expect(amend).not.toBeChecked();
    expect(summary).toHaveValue("Draft");
    expect(rpc.git.commit.message).not.toHaveBeenCalledWith(
      expect.objectContaining({ sha: "b2" }),
      expect.anything(),
    );
  });

  it("stops amending once the amend moves HEAD, before it's done", async () => {
    const { setLastCommit, amend } = renderForm();
    await userEvent.click(amend);
    await screen.findByDisplayValue("Commit a1");
    let finish!: () => void;
    rpc.git.commit.create.mockImplementationOnce(() => new Promise((r) => (finish = r)));
    await userEvent.click(screen.getByRole("button", { name: /Amend message/ }));
    expect(screen.getByLabelText("Commit message")).toBeDisabled();

    setLastCommit("b2");
    finish();
    expect(amend).not.toBeChecked();
    expect(rpc.git.commit.message).toHaveBeenCalledTimes(1);
  });

  it("turns off when there's no commit left to amend, and stays off", async () => {
    const { setLastCommit, summary, amend } = renderForm({ stagedCount: 1 });
    await userEvent.click(amend);
    await screen.findByDisplayValue("Commit a1");

    setLastCommit(undefined);
    expect(amend).not.toBeChecked();
    expect(amend).toBeDisabled();
    await userEvent.type(summary, "First commit");
    expect(screen.getByRole("button", { name: /Commit 1 file/ })).toBeEnabled();

    setLastCommit("c3");
    expect(amend).not.toBeChecked();
    expect(screen.getByRole("button", { name: /Commit 1 file/ })).toBeEnabled();
  });

  it("warns when the last commit is already on a remote branch", async () => {
    rpc.git.commit.pushedTo.mockResolvedValueOnce("origin/side");
    const { amend } = renderForm();
    await userEvent.click(amend);
    expect(await screen.findByText(/already on origin\/side/)).toBeInTheDocument();
  });

  it("doesn't call a failed push check an error", async () => {
    rpc.git.commit.pushedTo.mockRejectedValueOnce(new Error("for-each-ref failed"));
    const { amend } = renderForm();
    await userEvent.click(amend);
    await screen.findByDisplayValue("Commit a1");
    await vi.waitFor(() => expect(rpc.git.commit.pushedTo).toHaveBeenCalled());
    expect(screen.queryByText("for-each-ref failed")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Amend message/ })).toBeEnabled();
  });

  it("can't amend before the first commit", () => {
    const { amend } = renderForm({ lastCommit: null });
    expect(amend).toBeDisabled();
  });
});
