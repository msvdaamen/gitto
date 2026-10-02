import GitCommitHorizontal from "lucide-solid/icons/git-commit-horizontal";
import PenLine from "lucide-solid/icons/pen-line";
import { createSignal, Show } from "solid-js";

import { Button } from "@/components/ui/button";
import { FormField } from "@/components/ui/form-field";
import { Kbd } from "@/components/ui/kbd";
import { useCommitMessage, useCreateCommit } from "@/git/queries/commit";

interface Message {
  summary: string;
  description: string;
}

const emptyMessage: Message = { summary: "", description: "" };

/**
 * Commits what's staged in the repository, or amends the last commit with it. Amending loads the
 * last commit's message to edit, and puts back the message being written when it's turned off.
 */
export function CommitForm(props: {
  repositoryId: string;
  stagedCount: number;
  /** The commit HEAD points at, to amend; `undefined` before the first commit. */
  lastCommit: string | undefined;
  /** The upstream branch the last commit has already been pushed to, if any. */
  pushedTo: string | undefined;
}) {
  const commit = useCreateCommit(() => props.repositoryId);
  const loadMessage = useCommitMessage(() => props.repositoryId);
  const [message, setMessage] = createSignal(emptyMessage);
  const [amend, setAmend] = createSignal(false);
  const [loading, setLoading] = createSignal(false);
  const [loadError, setLoadError] = createSignal<Error>();
  // What was being written before amending, to put back when amending is turned off.
  let draft = emptyMessage;

  const canCommit = () =>
    !!message().summary.trim() &&
    (amend() || props.stagedCount > 0) &&
    !loading() &&
    !commit.isPending;

  // Counts toggles, so a message that finishes loading after another toggle is dropped.
  let toggles = 0;

  async function toggleAmend(on: boolean) {
    const toggle = ++toggles;
    setLoadError(undefined);
    setLoading(false);
    if (!on) {
      setAmend(false);
      setMessage(draft);
      return;
    }
    const sha = props.lastCommit;
    if (!sha) return;
    draft = message();
    setAmend(true);
    setLoading(true);
    try {
      const last = await loadMessage(sha);
      if (toggle === toggles) setMessage(last);
    } catch (error) {
      if (toggle !== toggles) return;
      setLoadError(error as Error);
      setAmend(false);
    } finally {
      if (toggle === toggles) setLoading(false);
    }
  }

  function submit() {
    if (!canCommit()) return;
    const summary = message().summary.trim();
    const body = message().description.trim();
    commit.mutate(
      { message: body ? `${summary}\n\n${body}` : summary, amend: amend() },
      {
        onSuccess: () => {
          draft = emptyMessage;
          setMessage(emptyMessage);
          setAmend(false);
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
        onChange={(summary) => setMessage((current) => ({ ...current, summary }))}
      />
      <FormField
        label="Description"
        hint="optional"
        placeholder="Add more context…"
        rows={3}
        value={message().description}
        onChange={(description) => setMessage((current) => ({ ...current, description }))}
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
          onChange={(event) => void toggleAmend(event.currentTarget.checked)}
        />
        Amend last commit
      </label>
      <Show when={amend() && props.pushedTo}>
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
      <Show when={commit.error ?? loadError()} keyed>
        {(error) => (
          <p class="m-0 mt-2.5 text-[11.5px] whitespace-pre-wrap text-coral">{error.message}</p>
        )}
      </Show>
    </form>
  );
}
