import { cn } from "cn";
import Archive from "lucide-solid/icons/archive";
import { Show, Suspense } from "solid-js";

import { useStashFiles } from "@/git/queries/stash";
import type { StashRow } from "@/git/rows";
import { useRelativeTime } from "@/hooks/relative-time";

import { HistoryGraph } from "./history-graph";
import { HistoryOption, RowKindBadge, RowTotals, type HistoryRowProps } from "./history-option";

/** A stash in the history: a box in the graph, with a dashed line to the commit it was made on. */
export function HistoryStashRow(
  props: HistoryRowProps & {
    stash: StashRow;
    /** Whether a search is active; the graph then only shows the node, like a commit's row. */
    searching: boolean;
  },
) {
  const ago = useRelativeTime();

  return (
    <HistoryOption
      selected={props.selected}
      position={props.position}
      onSelect={props.onSelect}
      class={cn(
        "border-border-soft bg-transparent hover:bg-panel-hover",
        props.selected &&
          "bg-[linear-gradient(90deg,color-mix(in_srgb,var(--amber)_18%,transparent),color-mix(in_srgb,var(--amber)_5%,transparent))] text-text-soft shadow-[inset_2px_0_var(--amber)]",
      )}
    >
      <RowKindBadge icon={Archive} label="Stash" />
      <div class="h-full overflow-x-clip">
        <Show when={props.stash.graph}>
          {(row) => <HistoryGraph row={row()} stash nodeOnly={props.searching} />}
        </Show>
      </div>
      <span class="flex min-w-0 items-center justify-between gap-[7px]">
        <strong class="truncate text-[13px] font-[570] text-text-soft italic">
          {props.stash.message}
        </strong>
        <Show when={props.selected && props.detailed}>
          <Suspense>
            <RowTotals
              repositoryId={props.stash.repositoryId}
              sha={props.stash.sha}
              useFiles={useStashFiles}
            />
          </Suspense>
        </Show>
      </span>
      <span class="truncate text-[11.5px]">{ago(props.stash.createdAt)}</span>
    </HistoryOption>
  );
}
