import GitCommitHorizontal from "lucide-solid/icons/git-commit-horizontal";
import { createSignal, Show } from "solid-js";

import { Button } from "@/components/ui/button";
import { FormField } from "@/components/ui/form-field";
import { Kbd } from "@/components/ui/kbd";
import { useCreateCommit } from "@/git/queries/commit";

/** Commits what's staged in the repository. */
export function CommitForm(props: { repositoryId: string; stagedCount: number }) {
  const commit = useCreateCommit(() => props.repositoryId);
  const [summary, setSummary] = createSignal("");
  const [description, setDescription] = createSignal("");

  const canCommit = () => !!summary().trim() && props.stagedCount > 0 && !commit.isPending;

  function submit() {
    if (!canCommit()) return;
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
        value={summary()}
        onChange={setSummary}
      />
      <FormField
        label="Description"
        hint="optional"
        placeholder="Add more context…"
        rows={3}
        value={description()}
        onChange={setDescription}
      />
      <Button
        type="submit"
        variant="primary"
        icon={GitCommitHorizontal}
        disabled={!canCommit()}
        class="h-8 w-full gap-[7px] rounded-[7px] text-[10px] font-[680] shadow-none"
      >
        {props.stagedCount > 0
          ? `Commit ${props.stagedCount} ${props.stagedCount === 1 ? "file" : "files"}`
          : "Nothing staged"}
        <Kbd class="ml-auto border-0 bg-transparent p-0 pr-[7px] text-[8px] text-[rgba(34,20,42,.65)]">
          ⌘ ↵
        </Kbd>
      </Button>
      <Show when={commit.error}>
        {(error) => (
          <p class="m-0 mt-2.5 text-[9px] whitespace-pre-wrap text-coral">{error().message}</p>
        )}
      </Show>
    </form>
  );
}
