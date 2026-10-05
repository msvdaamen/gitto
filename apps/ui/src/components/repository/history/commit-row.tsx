import { cn } from "cn";
import { Show, Suspense } from "solid-js";

import { useCommitFiles } from "@/git/queries/diff";
import type { CommitRow } from "@/git/rows";
import { useRelativeTime } from "@/hooks/relative-time";

import { HistoryGraph, laneColor } from "./history-graph";
import { HistoryOption, RowTotals, type HistoryRowProps } from "./history-option";
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
    /** Switches to a branch, by its full ref name, when its label is double-clicked. */
    onSwitchBranch: (ref: string) => void;
  },
) {
  const ago = useRelativeTime();

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
        onSwitch={props.onSwitchBranch}
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
        <Show when={props.selected && props.detailed}>
          <Suspense>
            <RowTotals
              repositoryId={props.commit.repositoryId}
              sha={props.commit.id}
              useFiles={useCommitFiles}
            />
          </Suspense>
        </Show>
      </span>
      <span class="truncate text-[11.5px]">{ago(props.commit.committedAt)}</span>
    </HistoryOption>
  );
}
