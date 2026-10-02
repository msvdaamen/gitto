import { cn } from "cn";
import PencilLine from "lucide-solid/icons/pencil-line";
import { Show, type JSX } from "solid-js";

import { toneClasses, type Tone } from "@/components/ui/tone";
import { useStatusNow } from "@/git/queries/status";
import { WIP_MESSAGE, type WipRow } from "@/git/rows";
import { headLabel } from "@/git/status";

import { HistoryGraph } from "./history-graph";
import { HistoryOption, type HistoryRowProps } from "./history-option";

/**
 * The uncommitted changes, set apart from the commits below: an amber, dashed-off row with a
 * hollow graph node that counts what's staged and what isn't.
 */
export function HistoryWipRow(
  props: HistoryRowProps & {
    row: WipRow;
    /** Whether a search is active; the graph then only shows the node, like a commit's row. */
    searching: boolean;
  },
) {
  // The row is only there once the status is in, so there's nothing to wait for.
  const status = useStatusNow(() => props.row.repositoryId);
  const counts = () => status().data?.counts ?? { staged: 0, unstaged: 0, conflicted: 0 };

  return (
    <HistoryOption
      selected={props.selected}
      position={props.position}
      onSelect={props.onSelect}
      class={cn(
        "border-dashed border-[color-mix(in_srgb,var(--amber)_45%,var(--border))] shadow-[inset_2px_0_var(--amber)]",
        props.selected
          ? "bg-[linear-gradient(90deg,color-mix(in_srgb,var(--amber)_24%,transparent),color-mix(in_srgb,var(--amber)_8%,transparent))]"
          : "bg-[color-mix(in_srgb,var(--amber-soft)_70%,transparent)] hover:bg-amber-soft",
      )}
    >
      <span class="flex">
        <span
          class={cn(
            "inline-flex items-center gap-1 rounded-sm border border-[color-mix(in_srgb,var(--amber)_40%,var(--border))] px-[5px] py-[3px] text-[8px] font-[680]",
            toneClasses.amber,
          )}
        >
          <PencilLine size={9} strokeWidth={2.4} />
          WIP
        </span>
      </span>
      <div class="h-full overflow-x-clip">
        <Show when={props.row.graph}>
          {(row) => <HistoryGraph row={row()} wip nodeOnly={props.searching} />}
        </Show>
      </div>
      <span class="col-span-3 flex min-w-0 items-center gap-2">
        <strong class="truncate text-[10.5px] font-[620] text-text italic">{WIP_MESSAGE}</strong>
        <Show when={counts().staged}>
          <WipCount tone="mint">{counts().staged} staged</WipCount>
        </Show>
        <Show when={counts().unstaged}>
          <WipCount tone="amber">{counts().unstaged} unstaged</WipCount>
        </Show>
        <Show when={counts().conflicted}>
          <WipCount tone="coral">{counts().conflicted} conflicted</WipCount>
        </Show>
        <span class="ml-auto shrink-0 text-[9px] text-faint">
          on {status().data && headLabel(status().data!.head)}
        </span>
      </span>
    </HistoryOption>
  );
}

function WipCount(props: { tone: Tone; children: JSX.Element }) {
  return (
    <small
      class={cn(
        "shrink-0 rounded-full px-1.5 py-0.5 text-[8px] font-[680]",
        toneClasses[props.tone],
      )}
    >
      {props.children}
    </small>
  );
}
