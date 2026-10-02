import { cn } from "cn";
import type { JSX } from "solid-js";

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
