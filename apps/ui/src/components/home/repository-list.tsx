import FolderOpen from "lucide-solid/icons/folder-open";
import Search from "lucide-solid/icons/search";
import { createMemo, createSignal, For, Show } from "solid-js";

import { Button } from "@/components/ui/button";
import { TextInput } from "@/components/ui/text-input";

import { RepositoryCard } from "./repository-card";
import type { RepositorySummary } from "./summaries";

/** The repositories added to Gitto, with a search box to filter them. */
export function RepositoryList(props: {
  summaries: RepositorySummary[];
  /** Whether the repositories have loaded: until then, there's no telling there are none. */
  loaded: boolean;
  onOpenRepository: () => void;
}) {
  const [query, setQuery] = createSignal("");
  const filtered = createMemo(() => {
    const needle = query().trim().toLowerCase();
    if (!needle) return props.summaries;
    return props.summaries.filter(({ repository, status, overview }) =>
      [
        repository.name,
        repository.path,
        status?.head.kind === "detached" ? "" : status?.head.name,
        overview?.remote?.url,
      ]
        .join(" ")
        .toLowerCase()
        .includes(needle),
    );
  });

  return (
    <section class="rounded-[11px] border border-border bg-panel p-5">
      <div class="mb-4 flex items-center justify-between gap-4 max-sm:flex-col max-sm:items-start max-sm:[&>label]:w-full">
        <div>
          <h2 class="mt-0 mb-0.75 text-[16px] tracking-[-.2px]">Repositories</h2>
          <p class="m-0 text-[13px] text-muted">The ones you've opened in Gitto</p>
        </div>
        <Show when={props.summaries.length}>
          <TextInput
            icon={Search}
            value={query()}
            onChange={setQuery}
            placeholder="Find a repository…"
            compact
          />
        </Show>
      </div>
      <div class="flex flex-col gap-1.5">
        <Show
          when={props.summaries.length || !props.loaded}
          fallback={
            <div class="grid h-40 place-items-center content-center gap-2 text-faint">
              <FolderOpen size={22} />
              <p class="m-0">No repositories yet. Open one to get started.</p>
              <Button variant="primary" icon={FolderOpen} onClick={props.onOpenRepository}>
                Open repository
              </Button>
            </div>
          }
        >
          <Show
            when={filtered().length || !props.loaded}
            fallback={
              <div class="grid h-40 place-items-center content-center gap-2 text-faint">
                <Search size={22} />
                <p class="m-0">No repositories match “{query()}”.</p>
              </div>
            }
          >
            <For each={filtered()}>{(summary) => <RepositoryCard summary={summary} />}</For>
          </Show>
        </Show>
      </div>
    </section>
  );
}
