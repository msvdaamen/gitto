import { cn } from "cn";
import type { LucideIcon } from "lucide-solid";
import type { JSX } from "solid-js";

import { LineStats } from "@/components/ui/line-stats";
import { toneClasses } from "@/components/ui/tone";
import type { useCommitFiles } from "@/git/queries/diff";

import { COLUMNS } from "./columns";

/**
 * A row's place in the list. Only the rows in view are rendered, so assistive tech needs to be told
 * how long the list is.
 */
export interface RowPosition {
  index: number;
  count: number;
}

/** What every history row gets from the table. */
export interface HistoryRowProps {
  selected: boolean;
  position: RowPosition;
  onSelect: () => void;
}

/**
 * A row of the history table as a selectable option, laid out in the table's columns; `class`
 * styles it.
 */
export function HistoryOption(props: HistoryRowProps & { class: string; children: JSX.Element }) {
  return (
    <button
      role="option"
      aria-selected={props.selected ? "true" : "false"}
      aria-posinset={props.position.index + 1}
      aria-setsize={props.position.count}
      class={cn(
        COLUMNS,
        "grid h-full w-full cursor-pointer items-center border-0 border-b p-0 text-left text-muted [&>span]:min-w-0 [&>span]:px-[9px]",
        props.class,
      )}
      onClick={() => props.onSelect()}
    >
      {props.children}
    </button>
  );
}

/** What a row that isn't a commit is, e.g. "WIP" or "Stash", in its branch / tag column. */
export function RowKindBadge(props: { icon: LucideIcon; label: string }) {
  return (
    <span class="flex">
      <span
        class={cn(
          "inline-flex items-center gap-1 rounded-sm border border-[color-mix(in_srgb,var(--amber)_40%,var(--border))] px-[5px] py-[3px] text-[10.5px] font-[680]",
          toneClasses.amber,
        )}
      >
        <props.icon size={9} strokeWidth={2.4} />
        {props.label}
      </span>
    </span>
  );
}

/**
 * The lines a commit or stash added and removed, as `useFiles` loads its files. Only shown for the
 * selected row, whose files are loaded anyway.
 */
export function RowTotals(props: {
  repositoryId: string;
  sha: string;
  useFiles: typeof useCommitFiles;
}) {
  // Read once: a row's kind, and so how its files load, doesn't change.
  const { totals } = props.useFiles(
    () => props.repositoryId,
    () => props.sha,
  );
  return (
    <LineStats
      additions={totals().additions}
      deletions={totals().deletions}
      class="shrink-0 text-[10.5px]"
    />
  );
}
