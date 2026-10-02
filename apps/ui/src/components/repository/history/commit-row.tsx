import { cn } from "cn";
import { Show, Suspense } from "solid-js";

import { LineStats } from "@/components/ui/line-stats";
import { useCommitFiles } from "@/git/queries/diff";
import type { CommitRow } from "@/git/rows";

import { HistoryGraph, laneColor } from "./history-graph";
import { HistoryOption, type HistoryRowProps } from "./history-option";
import { HistoryRefLabels } from "./ref-labels";

/** A commit in the history: its branches and tags, graph node (named for its author), message and date. */
export function HistoryCommitRow(
  props: HistoryRowProps & {
    commit: CommitRow;
    /**
     * The lowercase search, if any: a label it matches comes first, and the graph only shows the
     * commit's node, as its lines would lead to rows the search hides.
     */
    search: string;
  },
) {
  return (
    <HistoryOption
      selected={props.selected}
      position={props.position}
      onSelect={props.onSelect}
      class={cn(
        "group border-border-soft bg-transparent hover:bg-panel-hover",
        props.selected &&
          "bg-[linear-gradient(90deg,var(--primary-soft),color-mix(in_srgb,var(--primary-soft)_35%,transparent))] text-text-soft shadow-[inset_2px_0_var(--primary)]",
      )}
    >
      <HistoryRefLabels
        labels={props.commit.labels}
        search={props.search}
        color={laneColor(props.commit.graph?.column ?? 0)}
      />
      <div class="h-full overflow-x-clip">
        <Show when={props.commit.graph}>
          {(row) => (
            <HistoryGraph
              row={row()}
              nodeOnly={!!props.search}
              author={props.commit.author}
              initials={props.commit.initials}
              avatarColor={props.commit.avatarColor}
            />
          )}
        </Show>
      </div>
      <span class="flex min-w-0 items-center justify-between gap-[7px]">
        <strong class="truncate text-[13px] font-[570] text-text">{props.commit.message}</strong>
        {/* The author is otherwise only in the graph's tooltip, which assistive tech skips. */}
        <span class="sr-only">{`, by ${props.commit.author}`}</span>
        <Show when={props.selected}>
          <Suspense>
            <CommitTotals commit={props.commit} />
          </Suspense>
        </Show>
      </span>
      <span class="truncate text-[11.5px]">{props.commit.timestamp}</span>
    </HistoryOption>
  );
}

/** Line totals are only known for the selected commit, whose files are loaded anyway. */
function CommitTotals(props: { commit: CommitRow }) {
  const { totals } = useCommitFiles(
    () => props.commit.repositoryId,
    () => props.commit.id,
  );
  return (
    <LineStats
      additions={totals().additions}
      deletions={totals().deletions}
      class="shrink-0 text-[10.5px]"
    />
  );
}
