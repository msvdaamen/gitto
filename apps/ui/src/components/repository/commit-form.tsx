import GitCommitHorizontal from "lucide-solid/icons/git-commit-horizontal";
import { createSignal, Show } from "solid-js";

import { useCommit } from "@/git/commit";

/** Commits what's staged in the repository. */
export function CommitForm(props: { repositoryId: string }) {
  const commit = useCommit(() => props.repositoryId);
  const [summary, setSummary] = createSignal("");
  const [description, setDescription] = createSignal("");

  function submit() {
    if (!summary().trim() || commit.isPending) return;
    const body = description().trim();
    commit.mutate(body ? `${summary().trim()}\n\n${body}` : summary(), {
      onSuccess: () => {
        setSummary("");
        setDescription("");
      },
    });
  }

  return (
    <form
      class="p-3.5"
      onSubmit={(event) => {
        event.preventDefault();
        submit();
      }}
      onKeyDown={(event) => {
        if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) submit();
      }}
    >
      <label class="mb-2.5 flex flex-col gap-[5px] text-[9px] text-muted">
        <span class="flex justify-between">Commit message</span>
        <input
          class="w-full resize-y rounded-md border border-border bg-bg px-[9px] py-2 text-[9.5px] text-text outline-0 focus:border-primary focus:shadow-[0_0_0_3px_var(--primary-soft)]"
          placeholder="Summary of your changes"
          value={summary()}
          onInput={(event) => setSummary(event.currentTarget.value)}
        />
      </label>
      <label class="mb-2.5 flex flex-col gap-[5px] text-[9px] text-muted">
        <span class="flex justify-between">
          Description <small class="text-faint">optional</small>
        </span>
        <textarea
          class="w-full resize-y rounded-md border border-border bg-bg px-[9px] py-2 text-[9.5px] text-text outline-0 focus:border-primary focus:shadow-[0_0_0_3px_var(--primary-soft)]"
          placeholder="Add more context…"
          rows={3}
          value={description()}
          onInput={(event) => setDescription(event.currentTarget.value)}
        />
      </label>
      <button
        type="submit"
        disabled={!summary().trim() || commit.isPending}
        class="flex h-8 w-full cursor-pointer items-center justify-center gap-[7px] rounded-[7px] border border-[color-mix(in_srgb,var(--primary)_50%,var(--border))] bg-[linear-gradient(135deg,#c18deb,#9d72d7)] text-[10px] font-[680] text-[#21152a] disabled:cursor-default disabled:opacity-50 [&>kbd]:ml-auto [&>kbd]:pr-[7px] [&>kbd]:text-[8px] [&>kbd]:text-[rgba(34,20,42,.65)]"
      >
        <GitCommitHorizontal size={15} />
        Commit changes <kbd>⌘ ↵</kbd>
      </button>
      <Show when={commit.error}>
        {(error) => (
          <p class="m-0 mt-2.5 text-[9px] whitespace-pre-wrap text-coral">{error().message}</p>
        )}
      </Show>
    </form>
  );
}
