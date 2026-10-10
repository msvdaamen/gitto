import type { Commit, Stash } from "@gitto/git/types";

import { avatarColor, initials } from "@/lib/format";

import { computeGraph, type GraphCommit, type GraphRow } from "./graph";
import { NO_CHECKOUTS, toRefLabels, type Checkouts, type RefLabel } from "./ref-labels";

/** Row id of the uncommitted changes, shown above the history when there are any. */
export const WIP_ID = "wip";

/** Row id of the stash `sha`; set apart from commits' ids, which are their SHAs. */
export function stashRowId(sha: string): string {
  return `stash:${sha}`;
}

/** The SHA of the stash whose row id is `id`; `undefined` if it isn't a stash's. */
export function stashSha(id: string): string | undefined {
  return id.startsWith("stash:") ? id.slice("stash:".length) : undefined;
}

/** What the uncommitted changes' row says, in place of a commit message. */
export const WIP_MESSAGE = "Uncommitted changes";

/** A row in the history table: the uncommitted changes, a stash, or a commit. */
export type HistoryRow = WipRow | StashRow | CommitRow;

/** The uncommitted changes in the working directory, above the commits. */
export interface WipRow {
  kind: "wip";
  id: typeof WIP_ID;
  /** The repository the changes are in. */
  repositoryId: string;
  /** Its row in the history graph. */
  graph: GraphRow | undefined;
}

/** A stash, among the commits: above the one it was made on, with a dashed line to it. */
export interface StashRow {
  kind: "stash";
  /** See `stashRowId`. */
  id: string;
  /** The stash commit's full SHA. */
  sha: string;
  /** The repository the stash is in. */
  repositoryId: string;
  /** Its row in the history graph. */
  graph: GraphRow | undefined;
  /** What git named it, e.g. `WIP on main: 1a2b3c4 Fix the header`. */
  message: string;
  /** When it was made, in milliseconds since the epoch. */
  createdAt: number;
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
  /** The commit date, which the history is sorted by, in milliseconds since the epoch. */
  committedAt: number;
  /** The branches and tags pointing at it (see `toRefLabels`). */
  labels: RefLabel[];
}

/** What the history lists below the uncommitted changes: the log's commits, and the stashes. */
export type HistoryEntry = { kind: "commit"; commit: Commit } | { kind: "stash"; stash: Stash };

/**
 * The log, newest first, with the stashes among its commits: each just above the commit it was made
 * on, where its line to it is one row long and so never pushes other branches aside, as a line
 * past their tips would. One made on a commit that isn't in the log (a deleted branch's, say) goes
 * by when it was made, without a line; one older than every commit in it is left out, as it belongs
 * further back than the log goes. Stashes above the same commit are newest first.
 */
export function withStashes(log: Commit[], stashes: readonly Stash[]): HistoryEntry[] {
  if (!stashes.length) return log.map((commit) => ({ kind: "commit", commit }));
  const positions = new Map(log.map((commit, index) => [commit.sha, index]));
  // The stashes to list just above each commit.
  const above = new Map<number, Stash[]>();
  // Newest first (as listed, unless the clock was turned back), so the first commit made no later
  // than each is found in one pass down the log: it's never above the one for a newer stash.
  const byDate = stashes.toSorted((a, b) => b.createdAt - a.createdAt);
  let byDatePlace = 0;
  for (const stash of byDate) {
    while (byDatePlace < log.length && log[byDatePlace]!.committedAt > stash.createdAt) {
      byDatePlace++;
    }
    const place = positions.get(stash.base) ?? byDatePlace;
    if (place === log.length) continue;
    const stashesAbove = above.get(place);
    if (stashesAbove) stashesAbove.push(stash);
    else above.set(place, [stash]);
  }
  return log.flatMap((commit, index) => [
    ...(above.get(index) ?? []).map((stash): HistoryEntry => ({ kind: "stash", stash })),
    { kind: "commit", commit },
  ]);
}

/**
 * Lays out the history graph: the commits and stashes, below the uncommitted changes when there are
 * any. Those get a dashed line to the commit they're based on, `head`, and each stash one to the
 * commit it was made on, if it's in the history.
 */
export function historyGraph(
  entries: HistoryEntry[],
  hasChanges: boolean,
  head: string | undefined,
): GraphRow[] {
  const shas = new Set(
    entries.flatMap((entry) => (entry.kind === "commit" ? [entry.commit.sha] : [])),
  );
  const commits: GraphCommit[] = entries.map((entry) =>
    entry.kind === "commit"
      ? entry.commit
      : {
          sha: entry.stash.sha,
          parents: shas.has(entry.stash.base) ? [entry.stash.base] : [],
          dashed: true,
          aside: true,
        },
  );
  if (!hasChanges) return computeGraph(commits);
  const parents = head && shas.has(head) ? [head] : [];
  return computeGraph([{ sha: WIP_ID, parents, dashed: true }, ...commits]);
}

/**
 * Turns a repository's commits and stashes, plus the uncommitted changes if there are any, into
 * history table rows, with their rows of `graph`: the graph `historyGraph` laid out for the same
 * `entries` and `hasChanges`. The commits' labels say where the other worktrees are checked out
 * (`others`).
 */
export function toHistoryRows(
  repositoryId: string,
  entries: HistoryEntry[],
  hasChanges: boolean,
  graph: GraphRow[],
  others: Checkouts = NO_CHECKOUTS,
): HistoryRow[] {
  const offset = hasChanges ? 1 : 0;
  const rows = entries.map((entry, index) =>
    entry.kind === "commit"
      ? toCommitRow(repositoryId, entry.commit, graph[index + offset], others)
      : toStashRow(repositoryId, entry.stash, graph[index + offset]),
  );
  if (!hasChanges) return rows;
  return [{ kind: "wip", id: WIP_ID, repositoryId, graph: graph[0] }, ...rows];
}

function toStashRow(repositoryId: string, stash: Stash, graph: GraphRow | undefined): StashRow {
  return {
    kind: "stash",
    id: stashRowId(stash.sha),
    sha: stash.sha,
    repositoryId,
    graph,
    message: stash.message,
    createdAt: stash.createdAt,
  };
}

/** A commit's row; its labels say where the other worktrees are checked out (`others`). */
export function toCommitRow(
  repositoryId: string,
  commit: Commit,
  graph?: GraphRow,
  others: Checkouts = NO_CHECKOUTS,
): CommitRow {
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
    committedAt: commit.committedAt,
    labels: toRefLabels(commit.refs, commit.sha, others),
  };
}
