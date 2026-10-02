import { cn } from "cn";

/** The table's minimum width: its other columns', plus the graph's (see `graphWidth`). */
export const MIN_WIDTH = "min-w-[calc(560px+var(--graph-width))]";

/** The table's columns: branch / tag, graph, message, author and date. */
export const COLUMNS = cn(
  MIN_WIDTH,
  "grid-cols-[minmax(105px,.8fr)_var(--graph-width)_minmax(240px,2.2fr)_minmax(115px,.85fr)_100px]",
);
