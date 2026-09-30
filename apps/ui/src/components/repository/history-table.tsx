import LoaderCircle from "lucide-solid/icons/loader-circle";
import Search from "lucide-solid/icons/search";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import { createMemo, For, Show, Suspense } from "solid-js";

import { Avatar } from "@/components/ui/avatar";
import { EmptyState } from "@/components/ui/empty-state";
import { useChangedFiles } from "@/git/diff";
import { useHistory } from "@/git/history";
import type { Commit } from "@/types/git";

export function HistoryTable(props: {
  repositoryId: string;
  search: string;
  selectedId: string | undefined;
  onSelect: (id: string) => void;
}) {
  return (
    <main class="min-h-0 min-w-0 overflow-hidden bg-bg">
      <div class="grid h-[31px] min-w-[705px] grid-cols-[minmax(105px,.8fr)_82px_minmax(240px,2.2fr)_minmax(115px,.85fr)_100px_63px] items-center border-b border-border bg-bg-soft text-[8px] font-[720] tracking-[.055em] text-faint uppercase [&>span]:flex [&>span]:h-full [&>span]:items-center [&>span]:border-r [&>span]:border-border-soft [&>span]:px-[9px]">
        <span>Branch / tag</span>
        <span>Graph</span>
        <span>Commit message</span>
        <span>Author</span>
        <span>Date</span>
        <span>SHA</span>
      </div>
      <div
        class="h-[calc(100%_-_31px)] min-w-[705px] overflow-auto"
        role="listbox"
        aria-label="Commit history"
      >
        <Suspense fallback={<EmptyState icon={LoaderCircle} title="Loading history…" />}>
          <HistoryRows {...props} />
        </Suspense>
      </div>
    </main>
  );
}

function HistoryRows(props: {
  repositoryId: string;
  search: string;
  selectedId: string | undefined;
  onSelect: (id: string) => void;
}) {
  const history = useHistory(
    () => props.repositoryId,
    () => props.selectedId,
  );
  const visibleRows = createMemo(() => {
    const needle = props.search.trim().toLowerCase();
    return needle
      ? history
          .rows()
          .filter((commit) =>
            `${commit.message} ${commit.author} ${commit.id} ${commit.refs.join(" ")}`
              .toLowerCase()
              .includes(needle),
          )
      : history.rows();
  });

  return (
    <Show
      when={!history.log.error}
      fallback={
        <EmptyState icon={TriangleAlert} title="Couldn't load history" tone="error">
          {history.log.error?.message}
        </EmptyState>
      }
    >
      <Show
        when={visibleRows().length}
        fallback={
          <EmptyState icon={Search} title="No commits found">
            Try a different message, author, or SHA.
          </EmptyState>
        }
      >
        <For each={visibleRows()}>
          {(commit) => (
            <HistoryRow
              commit={commit}
              selected={history.selected()?.id === commit.id}
              onSelect={() => props.onSelect(commit.id)}
            />
          )}
        </For>
      </Show>
    </Show>
  );
}

function HistoryRow(props: { commit: Commit; selected: boolean; onSelect: () => void }) {
  return (
    <button
      role="option"
      aria-selected={props.selected ? "true" : "false"}
      class={`group grid h-[47px] w-full min-w-[705px] cursor-pointer grid-cols-[minmax(105px,.8fr)_82px_minmax(240px,2.2fr)_minmax(115px,.85fr)_100px_63px] items-center border-0 border-b border-border-soft bg-transparent p-0 text-left text-muted hover:bg-panel-hover [&>span]:min-w-0 [&>span]:px-[9px] ${props.selected ? "bg-[linear-gradient(90deg,var(--primary-soft),color-mix(in_srgb,var(--primary-soft)_35%,transparent))] text-text-soft shadow-[inset_2px_0_var(--primary)]" : ""} ${props.commit.isWip ? "bg-[color-mix(in_srgb,var(--amber-soft)_35%,transparent)]" : ""}`}
      onClick={props.onSelect}
    >
      <span class="flex gap-1 overflow-hidden">
        <For each={props.commit.refs.slice(0, 2)}>
          {(ref) => (
            <span
              class={`max-w-[78px] truncate rounded-sm border border-border bg-panel px-[5px] py-[3px] text-[8px] text-text-soft ${ref === "HEAD" || ref === "WIP" ? "border-[color-mix(in_srgb,var(--primary)_40%,var(--border))] bg-primary-soft text-primary-strong" : ref.startsWith("origin") ? "bg-blue-soft text-blue" : ""}`}
            >
              {ref}
            </span>
          )}
        </For>
      </span>
      <span
        class="flex h-full items-center font-mono text-[19px] font-bold tracking-[-5px] whitespace-pre [&>i]:w-[18px] [&>i]:not-italic"
        aria-label={`Graph ${props.commit.graph.join(" ")}`}
      >
        <For each={props.commit.graph}>
          {(cell, index) => (
            <i class={["text-primary-strong", "text-blue", "text-mint"][index() % 3]}>{cell}</i>
          )}
        </For>
      </span>
      <span class="flex min-w-0 items-center justify-between gap-[7px]">
        <strong class="truncate text-[10.5px] font-[570] text-text">{props.commit.message}</strong>
        <Show when={props.selected}>
          <Suspense>
            <CommitTotals commit={props.commit} />
          </Suspense>
        </Show>
      </span>
      <span class="flex items-center gap-[7px]">
        <Avatar initials={props.commit.initials} color={props.commit.avatarColor} />
        <span class="truncate text-[9.5px]">
          {props.commit.author.split(" ").map((part, index) => (
            <>
              {index > 0 && " "}
              <span>{part}</span>
            </>
          ))}
        </span>
      </span>
      <span class="truncate text-[9px]">{props.commit.timestamp}</span>
      <span class="font-mono text-[9px] text-faint">{props.commit.sha}</span>
    </button>
  );
}

/** Line totals are only known for the selected commit, whose files are loaded anyway. */
function CommitTotals(props: { commit: Commit }) {
  const { totals } = useChangedFiles(() => props.commit);
  return (
    <small class="flex items-center gap-[5px] text-[8px]">
      <span class="text-mint">+{totals().additions}</span>
      <span class="text-coral">−{totals().deletions}</span>
    </small>
  );
}
