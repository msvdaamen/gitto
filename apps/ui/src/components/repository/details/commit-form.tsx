import { useQuery } from "@tanstack/solid-query";
import GitCommitHorizontal from "lucide-solid/icons/git-commit-horizontal";
import PenLine from "lucide-solid/icons/pen-line";
import { createEffect, createMemo, createSignal, on, Show } from "solid-js";

import { Button } from "@/components/ui/button";
import { FormField } from "@/components/ui/form-field";
import { Kbd } from "@/components/ui/kbd";
import {
  editMessage,
  emptyMessage,
  joinMessage,
  splitMessage,
  type CommitMessage,
} from "@/git/message";
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
  // HEAD, told apart from the same commit in another repository, e.g. another worktree.
  const head = () => props.lastCommit && `${props.repositoryId} ${props.lastCommit}`;
  // The commit being amended (as `head`). Amending stops once HEAD moves to another commit: after
  // the amend, or when one is made or checked out elsewhere, so a stale message is never amended
  // onto it.
  const [amending, setAmending] = createSignal<string>();
  createEffect(on(head, () => setAmending(undefined), { defer: true }));
  // Checked against HEAD too, as the effect only runs once HEAD has moved.
  const amend = () => !!amending() && amending() === head();
  const [draft, setDraft] = createSignal(emptyMessage);
  // Edits to the amended message, kept for the commit they were made to while amending is toggled.
  const [edits, setEdits] = createSignal<{ head: string; message: CommitMessage }>();

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
  const loading = () => amend() && lastMessage.isPending;
  // Also locked if the message failed to load (the error says so), so no edits stand in for it.
  const locked = () => (amend() && !loaded()) || commit.isPending;
  const amendEdits = () => {
    const edited = edits();
    return edited && edited.head === head() ? edited.message : undefined;
  };

  const message = () => (amend() ? (amendEdits() ?? loaded() ?? emptyMessage) : draft());
  function edit(change: Partial<CommitMessage>) {
    const amended = head();
    if (amend() && amended) setEdits({ head: amended, message: { ...message(), ...change } });
    else setDraft((current) => ({ ...current, ...change }));
  }

  function toggleAmend(checked: boolean) {
    setAmending(checked ? head() : undefined);
    commit.reset();
  }

  // The message to amend with, once the last one has loaded: what wasn't edited of it exactly as
  // written, not as the form split it into two fields.
  const amendedMessage = () => {
    const original = loaded() && lastMessage.data;
    return original ? editMessage(original, message()) : undefined;
  };
  // Amending with nothing staged and the same message would only rewrite the commit's date.
  const amendChanges = () =>
    props.stagedCount > 0 || (!!amendedMessage() && amendedMessage() !== lastMessage.data);

  const canCommit = () =>
    !!message().summary.trim() &&
    // Once the push check is done too, so its warning is seen before amending.
    (amend() ? amendChanges() && !pushedTo.isPending : props.stagedCount > 0) &&
    !commit.isPending;

  function submit() {
    if (!canCommit()) return;
    const amended = amend() ? props.lastCommit : undefined;
    const text = (amended && amendedMessage()) || joinMessage(message());
    commit.mutate(
      { message: text, amend: amended },
      {
        onSuccess: () => {
          if (!amended) return setDraft(emptyMessage);
          setAmending(undefined);
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
        placeholder={loading() ? "Loading the last commit…" : "Summary of your changes"}
        value={message().summary}
        onChange={(summary) => edit({ summary })}
        readOnly={locked()}
      />
      <FormField
        label="Description"
        hint="optional"
        placeholder="Add more context…"
        rows={3}
        value={message().description}
        onChange={(description) => edit({ description })}
        readOnly={locked()}
      />
      <label class="mb-2.5 flex w-fit cursor-pointer items-center gap-[7px] text-[11.5px] text-muted has-disabled:cursor-default has-disabled:opacity-50">
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
        {(branch) => (
          <p class="m-0 mb-2.5 text-[11.5px] leading-[1.45] text-amber">
            The last commit is already on {branch()}; amending it means force-pushing.
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
      {/* Not whether it's been pushed: that only decides the warning, and doesn't stop amending. */}
      <Show when={commit.error ?? (amend() && lastMessage.error)} keyed>
        {(error) => (
          <p class="m-0 mt-2.5 text-[11.5px] whitespace-pre-wrap text-coral">{error.message}</p>
        )}
      </Show>
    </form>
  );
}
