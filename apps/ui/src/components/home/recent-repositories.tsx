import Search from "lucide-solid/icons/search";
import { createMemo, createSignal, For, Show } from "solid-js";

import { TextInput } from "@/components/ui/text-input";
import type { RepositorySummary } from "@/data/mock-data";

import { RepositoryCard } from "./repository-card";

/** The recently opened repositories, with a search box to filter them. */
export function RecentRepositories(props: {
  repositories: RepositorySummary[];
  onOpenRepository: () => void;
}) {
  const [query, setQuery] = createSignal("");
  const filtered = createMemo(() => {
    const needle = query().trim().toLowerCase();
    if (!needle) return props.repositories;
    return props.repositories.filter((repository) =>
      `${repository.name} ${repository.owner} ${repository.path} ${repository.branch}`
        .toLowerCase()
        .includes(needle),
    );
  });

  return (
    <section class="rounded-[11px] border border-border bg-panel p-5">
      <div class="mb-4 flex items-center justify-between gap-4 max-sm:flex-col max-sm:items-start max-sm:[&>label]:w-full">
        <div>
          <h2 class="mt-0 mb-0.75 text-[15px] tracking-[-.2px]">Recent repositories</h2>
          <p class="m-0 text-[10.5px] text-muted">Your latest local workspaces</p>
        </div>
        <TextInput
          icon={Search}
          value={query()}
          onChange={setQuery}
          placeholder="Find a repository…"
          compact
        />
      </div>
      <div class="flex flex-col gap-1.5">
        <Show
          when={filtered().length}
          fallback={
            <div class="grid h-40 place-items-center content-center gap-2 text-faint">
              <Search size={22} />
              <p class="m-0">No repositories match “{query()}”.</p>
            </div>
          }
        >
          <For each={filtered()}>
            {(repository) => (
              <RepositoryCard repository={repository} onOpen={props.onOpenRepository} />
            )}
          </For>
        </Show>
      </div>
    </section>
  );
}
