import LoaderCircle from "lucide-solid/icons/loader-circle";
import PencilLine from "lucide-solid/icons/pencil-line";
import Search from "lucide-solid/icons/search";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import { createMemo, For, Show, Suspense, type JSX } from "solid-js";

import { Avatar } from "@/components/ui/avatar";
import { EmptyState } from "@/components/ui/empty-state";
import { useCommitFiles } from "@/git/diff";
import { useHistory } from "@/git/history";
import { useStatus } from "@/git/status";
import type { Commit } from "@/types/git";

export function HistoryTable(props: {
  repositoryId: string;
  search: string;
  selectedId: string | undefined;
  onSelect: (id: string) => void;
}) {
  return (
    <main class="min-h-0 min-w-0 overflow-auto bg-bg">
      <div class="sticky top-0 z-10 grid h-[31px] min-w-[642px] grid-cols-[minmax(105px,.8fr)_82px_minmax(240px,2.2fr)_minmax(115px,.85fr)_100px] items-center border-b border-border bg-bg-soft text-[8px] font-[720] tracking-[.055em] text-faint uppercase [&>span]:flex [&>span]:h-full [&>span]:items-center [&>span]:border-r [&>span]:border-border-soft [&>span]:px-[9px]">
        <span>Branch / tag</span>
        <span>Graph</span>
        <span>Commit message</span>
        <span>Author</span>
        <span>Date</span>
      </div>
      <div class="min-w-[642px]" role="listbox" aria-label="Commit history">
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
            <Show
              when={commit.isWip}
              fallback={
                <HistoryRow
                  commit={commit}
                  selected={history.selected()?.id === commit.id}
                  onSelect={() => props.onSelect(commit.id)}
                />
              }
            >
              <WipRow
                commit={commit}
                selected={history.selected()?.id === commit.id}
                onSelect={() => props.onSelect(commit.id)}
              />
            </Show>
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
      class={`group grid h-[47px] w-full min-w-[642px] cursor-pointer grid-cols-[minmax(105px,.8fr)_82px_minmax(240px,2.2fr)_minmax(115px,.85fr)_100px] items-center border-0 border-b border-border-soft bg-transparent p-0 text-left text-muted hover:bg-panel-hover [&>span]:min-w-0 [&>span]:px-[9px] ${props.selected ? "bg-[linear-gradient(90deg,var(--primary-soft),color-mix(in_srgb,var(--primary-soft)_35%,transparent))] text-text-soft shadow-[inset_2px_0_var(--primary)]" : ""}`}
      onClick={props.onSelect}
    >
      <span class="flex gap-1 overflow-hidden">
        <For each={props.commit.refs.slice(0, 2)}>
          {(ref) => (
            <span
              class={`max-w-[78px] truncate rounded-sm border border-border bg-panel px-[5px] py-[3px] text-[8px] text-text-soft ${ref === "HEAD" ? "border-[color-mix(in_srgb,var(--primary)_40%,var(--border))] bg-primary-soft text-primary-strong" : ref.startsWith("origin") ? "bg-blue-soft text-blue" : ""}`}
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
    </button>
  );
}

/**
 * The uncommitted changes, set apart from the commits below: an amber, dashed-off row with a
 * hollow graph node that counts what's staged and what isn't.
 */
function WipRow(props: { commit: Commit; selected: boolean; onSelect: () => void }) {
  const status = useStatus(() => props.commit.repositoryId);
  const counts = createMemo(() => {
    const files = status.data?.files ?? [];
    return {
      staged: files.filter((file) => file.staged && file.staged !== "conflicted").length,
      unstaged: files.filter((file) => file.unstaged && file.unstaged !== "conflicted").length,
      conflicted: files.filter((file) => file.staged === "conflicted").length,
    };
  });

  return (
    <button
      role="option"
      aria-selected={props.selected ? "true" : "false"}
      class={`grid h-[47px] w-full min-w-[642px] cursor-pointer grid-cols-[minmax(105px,.8fr)_82px_minmax(240px,2.2fr)_minmax(115px,.85fr)_100px] items-center border-0 border-b border-dashed border-[color-mix(in_srgb,var(--amber)_45%,var(--border))] p-0 text-left text-muted shadow-[inset_2px_0_var(--amber)] [&>span]:min-w-0 [&>span]:px-[9px] ${props.selected ? "bg-[linear-gradient(90deg,color-mix(in_srgb,var(--amber)_24%,transparent),color-mix(in_srgb,var(--amber)_8%,transparent))]" : "bg-[color-mix(in_srgb,var(--amber-soft)_70%,transparent)] hover:bg-amber-soft"}`}
      onClick={props.onSelect}
    >
      <span class="flex">
        <span class="inline-flex items-center gap-1 rounded-sm border border-[color-mix(in_srgb,var(--amber)_40%,var(--border))] bg-amber-soft px-[5px] py-[3px] text-[8px] font-[680] text-amber">
          <PencilLine size={9} strokeWidth={2.4} />
          WIP
        </span>
      </span>
      <span
        class="flex h-full items-center font-mono text-[19px] font-bold tracking-[-5px] whitespace-pre [&>i]:w-[18px] [&>i]:not-italic"
        aria-hidden="true"
      >
        <For each={props.commit.graph}>
          {(cell) => <i class={cell === "○" ? "text-amber" : "text-faint"}>{cell}</i>}
        </For>
      </span>
      <span class="col-span-3 flex min-w-0 items-center gap-2">
        <strong class="truncate text-[10.5px] font-[620] text-text italic">
          {props.commit.message}
        </strong>
        <Show when={counts().staged}>
          <WipCount class="bg-mint-soft text-mint">{counts().staged} staged</WipCount>
        </Show>
        <Show when={counts().unstaged}>
          <WipCount class="bg-amber-soft text-amber">{counts().unstaged} unstaged</WipCount>
        </Show>
        <Show when={counts().conflicted}>
          <WipCount class="bg-coral-soft text-coral">{counts().conflicted} conflicted</WipCount>
        </Show>
        <span class="ml-auto shrink-0 text-[9px] text-faint">
          on {status.data?.branch ?? "detached HEAD"}
        </span>
      </span>
    </button>
  );
}

function WipCount(props: { class: string; children: JSX.Element }) {
  return (
    <small class={`shrink-0 rounded-full px-1.5 py-0.5 text-[8px] font-[680] ${props.class}`}>
      {props.children}
    </small>
  );
}

/** Line totals are only known for the selected commit, whose files are loaded anyway. */
function CommitTotals(props: { commit: Commit }) {
  const { totals } = useCommitFiles(() => props.commit);
  return (
    <small class="flex items-center gap-[5px] text-[8px]">
      <span class="text-mint">+{totals().additions}</span>
      <span class="text-coral">−{totals().deletions}</span>
    </small>
  );
}
