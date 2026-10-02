import { useQuery } from "@tanstack/solid-query";
import GitCommitHorizontal from "lucide-solid/icons/git-commit-horizontal";
import PenLine from "lucide-solid/icons/pen-line";
import { createMemo, createSignal, Show } from "solid-js";

import { Button } from "@/components/ui/button";
import { FormField } from "@/components/ui/form-field";
import { Kbd } from "@/components/ui/kbd";
import { emptyMessage, joinMessage, splitMessage, type CommitMessage } from "@/git/message";
import { commitMessageQuery, pushedToQuery, useCreateCommit } from "@/git/queries/commit";

/**
 * Commits what's staged in the repository, or amends the last commit with it. The new commit's
 * message and the amended one are kept apart, so switching between them loses neither.
 */
export function CommitForm(props: {
  repositoryId: string;
  stagedCount: number;
  /** The commit to amend, HEAD's; `undefined` before the first commit. */
  lastCommit: string | undefined;
}) {
  const commit = useCreateCommit(() => props.repositoryId);
  const [amendChecked, setAmendChecked] = createSignal(false);
  // Turns itself off when there's no commit left to amend, e.g. after `git switch --orphan`.
  const amend = () => amendChecked() && !!props.lastCommit;
  const [draft, setDraft] = createSignal(emptyMessage);
  // Edits to the last commit's message, for the commit it was loaded from: if HEAD moves, the new
  // last commit's message is loaded instead, so it's never amended with another one's.
  const [edits, setEdits] = createSignal<{ sha: string; message: CommitMessage }>();

  const lastMessage = useQuery(() => ({
    ...commitMessageQuery(props.repositoryId, props.lastCommit ?? ""),
    enabled: amend(),
  }));
  const pushedTo = useQuery(() => ({
    ...pushedToQuery(props.repositoryId, props.lastCommit ?? ""),
    enabled: amend(),
  }));
  // Read only once loaded: reading `data` while it loads would suspend the details panel.
  const loaded = createMemo(() =>
    lastMessage.isSuccess ? splitMessage(lastMessage.data) : undefined,
  );
  const amendEdits = () => {
    const edited = edits();
    return edited && edited.sha === props.lastCommit ? edited.message : undefined;
  };

  const message = () => (amend() ? (amendEdits() ?? loaded() ?? emptyMessage) : draft());
  function edit(change: Partial<CommitMessage>) {
    const sha = props.lastCommit;
    if (amend() && sha) setEdits({ sha, message: { ...message(), ...change } });
    else setDraft((current) => ({ ...current, ...change }));
  }

  function toggleAmend(on: boolean) {
    setAmendChecked(on);
    commit.reset();
  }

  const canCommit = () =>
    !!message().summary.trim() &&
    (amend() ? !!loaded() : props.stagedCount > 0) &&
    !commit.isPending;

  function submit() {
    if (!canCommit()) return;
    const amending = amend();
    // An untouched message is amended as written, not as the form splits it into two fields.
    const text = (amending && !amendEdits() && lastMessage.data) || joinMessage(message());
    commit.mutate(
      { message: text, amend: amending },
      {
        onSuccess: () => {
          if (!amending) return setDraft(emptyMessage);
          setAmendChecked(false);
          setEdits(undefined);
        },
      },
    );
  }

  const label = () => {
    const files = `${props.stagedCount} ${props.stagedCount === 1 ? "file" : "files"}`;
    if (amend()) return props.stagedCount > 0 ? `Amend with ${files}` : "Amend message";
    return props.stagedCount > 0 ? `Commit ${files}` : "Nothing staged";
  };

  return (
    <form
      class="shrink-0 p-3.5"
      onSubmit={(event) => {
        event.preventDefault();
        submit();
      }}
      onKeyDown={(event) => {
        if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) submit();
      }}
    >
      <FormField
        label="Commit message"
        placeholder="Summary of your changes"
        value={message().summary}
        onChange={(summary) => edit({ summary })}
        disabled={amend() && !loaded()}
      />
      <FormField
        label="Description"
        hint="optional"
        placeholder="Add more context…"
        rows={3}
        value={message().description}
        onChange={(description) => edit({ description })}
        disabled={amend() && !loaded()}
      />
      <label
        class="mb-2.5 flex w-fit cursor-pointer items-center gap-[7px] text-[11.5px] text-muted has-disabled:cursor-default has-disabled:opacity-50"
        title={props.lastCommit ? undefined : "There's no commit to amend yet"}
      >
        <input
          type="checkbox"
          class="m-0 size-3.5 cursor-[inherit] accent-primary"
          checked={amend()}
          disabled={!props.lastCommit || commit.isPending}
          onChange={(event) => toggleAmend(event.currentTarget.checked)}
        />
        Amend last commit
      </label>
      <Show when={amend() && pushedTo.isSuccess && pushedTo.data}>
        {(upstream) => (
          <p class="m-0 mb-2.5 text-[11.5px] leading-[1.45] text-amber">
            The last commit is already on {upstream()}; amending it means force-pushing.
          </p>
        )}
      </Show>
      <Button
        type="submit"
        variant="primary"
        icon={amend() ? PenLine : GitCommitHorizontal}
        disabled={!canCommit()}
        class="h-8 w-full gap-[7px] rounded-[7px] text-[12.5px] font-[680] shadow-none"
      >
        {label()}
        <Kbd class="ml-auto border-0 bg-transparent p-0 pr-[7px] text-[10.5px] text-[rgba(34,20,42,.65)]">
          ⌘ ↵
        </Kbd>
      </Button>
      <Show when={commit.error ?? (amend() && (lastMessage.error ?? pushedTo.error))} keyed>
        {(error) => (
          <p class="m-0 mt-2.5 text-[11.5px] whitespace-pre-wrap text-coral">{error.message}</p>
        )}
      </Show>
    </form>
  );
}
