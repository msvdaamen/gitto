import { refLabelMatches } from "./ref-labels";
import { WIP_MESSAGE, type HistoryRow } from "./rows";

/** Lowercases and trims what was typed in the history search; empty means no search. */
export function searchNeedle(search: string): string {
  return search.trim().toLowerCase();
}

/**
 * Whether a history row matches `needle` (see `searchNeedle`): a commit by its message, author,
 * SHA or one of its branches and tags; a stash by its message or SHA; the uncommitted changes by
 * what their row says.
 */
export function matchesSearch(row: HistoryRow, needle: string): boolean {
  if (row.kind === "wip") return WIP_MESSAGE.toLowerCase().includes(needle);
  if (row.kind === "stash") return `${row.message} ${row.sha}`.toLowerCase().includes(needle);
  return (
    `${row.message} ${row.author} ${row.id}`.toLowerCase().includes(needle) ||
    row.labels.some((label) => refLabelMatches(label, needle))
  );
}
