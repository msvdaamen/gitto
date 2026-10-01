import type { Commit } from "@gitto/git/types";

import { avatarColor, initials, relativeTime } from "@/lib/format";

import { computeGraph, type GraphRow } from "./graph";
import { toRefLabels, type RefLabel } from "./ref-labels";

/** Row id of the uncommitted changes, shown above the history when there are any. */
export const WIP_ID = "wip";

/** What the uncommitted changes' row says, in place of a commit message. */
export const WIP_MESSAGE = "Uncommitted changes";

/** A row in the history table: the uncommitted changes, or a commit. */
export type HistoryRow = WipRow | CommitRow;

/** The uncommitted changes in the working directory, above the commits. */
export interface WipRow {
  kind: "wip";
  id: typeof WIP_ID;
  /** The repository the changes are in. */
  repositoryId: string;
  /** Its row in the history graph. */
  graph: GraphRow | undefined;
}

/** A commit as the UI shows it. */
export interface CommitRow {
  kind: "commit";
  /** The full SHA. */
  id: string;
  /** The repository the commit is in, so follow-up requests (like its files) go there. */
  repositoryId: string;
  shortSha: string;
  /** Its row in the history graph; only rows in the history table have one. */
  graph?: GraphRow;
  message: string;
  description?: string;
  author: string;
  initials: string;
  avatarColor: string;
  /** The commit date, which the history is sorted by, relative to now. */
  timestamp: string;
  /** The branches and tags pointing at it (see `toRefLabels`). */
  labels: RefLabel[];
}

/**
 * Lays out the history graph: the log, below the uncommitted changes when there are any. Those
 * get a dashed line to the commit they're based on, `head`, if it's in the log.
 */
export function historyGraph(
  log: Commit[],
  hasChanges: boolean,
  head: string | undefined,
): GraphRow[] {
  if (!hasChanges) return computeGraph(log);
  const parents = head && log.some((commit) => commit.sha === head) ? [head] : [];
  return computeGraph([{ sha: WIP_ID, parents, dashed: true }, ...log]);
}

/**
 * Turns a repository's log, plus the uncommitted changes if there are any, into history table
 * rows, with their rows of `graph`: the graph `historyGraph` laid out for the same `log` and
 * `hasChanges`.
 */
export function toHistoryRows(
  repositoryId: string,
  log: Commit[],
  hasChanges: boolean,
  graph: GraphRow[],
): HistoryRow[] {
  const offset = hasChanges ? 1 : 0;
  const rows = log.map((commit, index) => toCommitRow(repositoryId, commit, graph[index + offset]));
  if (!hasChanges) return rows;
  return [{ kind: "wip", id: WIP_ID, repositoryId, graph: graph[0] }, ...rows];
}

export function toCommitRow(repositoryId: string, commit: Commit, graph?: GraphRow): CommitRow {
  return {
    kind: "commit",
    id: commit.sha,
    repositoryId,
    shortSha: commit.sha.slice(0, 7),
    graph,
    message: commit.subject,
    description: commit.body || undefined,
    author: commit.authorName,
    initials: initials(commit.authorName),
    avatarColor: avatarColor(commit.authorEmail),
    timestamp: relativeTime(commit.committedAt),
    labels: toRefLabels(commit.refs),
  };
}
