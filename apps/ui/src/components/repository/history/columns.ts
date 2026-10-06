import { cn } from "cn";

/** Height of a history row, including its 1px bottom border. */
export const ROW_HEIGHT = 38;

/** The table's minimum width: its other columns', plus the graph's (see `graphWidth`). */
export const MIN_WIDTH = "min-w-[calc(445px+var(--graph-width))]";

/** The table's columns: branch / tag, graph, message and date. */
export const COLUMNS = cn(
  MIN_WIDTH,
  "grid-cols-[minmax(105px,.8fr)_var(--graph-width)_minmax(240px,2.2fr)_100px]",
);
