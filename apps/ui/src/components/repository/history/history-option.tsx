import { cn } from "cn";
import type { LucideIcon } from "lucide-solid";
import type { JSX } from "solid-js";

import { LineStats } from "@/components/ui/line-stats";
import { toneClasses } from "@/components/ui/tone";
import { useCommitFiles } from "@/git/queries/diff";
import { useStashFiles } from "@/git/queries/stash";

import { COLUMNS } from "./columns";

/**
 * A row's place in the list. Only the rows in view are rendered, so assistive tech needs to be told
 * how long the list is.
 */
export interface RowPosition {
  index: number;
  count: number;
}

/** The id of the option at `index` in the list, for the list to say which one is selected. */
export function optionId(index: number): string {
  return `history-option-${index}`;
}

/** What every history row gets from the table. */
export interface HistoryRowProps {
  selected: boolean;
  /**
   * Whether the details show this row too: they follow the selection, but skip the rows it only
   * passes while it moves quickly. What a row loads to show more of itself waits for that.
   */
  detailed: boolean;
  position: RowPosition;
  onSelect: () => void;
}

/**
 * A row of the history table as a selectable option, laid out in the table's columns; `class`
 * styles it. Not focused itself: the list has the focus, and moves the selection with the keyboard.
 * The selected one is outlined while the keyboard does.
 */
export function HistoryOption(
  props: Omit<HistoryRowProps, "detailed"> & { class: string; children: JSX.Element },
) {
  return (
    <button
      id={optionId(props.position.index)}
      role="option"
      tabIndex={-1}
      aria-selected={props.selected ? "true" : "false"}
      aria-posinset={props.position.index + 1}
      aria-setsize={props.position.count}
      class={cn(
        COLUMNS,
        "grid h-full w-full cursor-pointer items-center border-0 border-b p-0 text-left text-muted [&>span]:min-w-0 [&>span]:px-[9px]",
        props.selected &&
          "group-focus-visible/history:outline-2 group-focus-visible/history:-outline-offset-2 group-focus-visible/history:outline-primary",
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
 * The lines a commit or stash added and removed, as its files load. Only shown for the selected
 * row once the details show it (`HistoryRowProps.detailed`), whose files are loaded anyway: for
 * every row an arrow key held down passes, it would run git once per row.
 */
export function RowTotals(props: {
  repositoryId: string;
  sha: string;
  /** What the row is, which says how its files load. */
  kind: "commit" | "stash";
}) {
  // Read once: a row's kind, and so how its files load, doesn't change.
  const useFiles = props.kind === "stash" ? useStashFiles : useCommitFiles;
  const { totals } = useFiles(
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
