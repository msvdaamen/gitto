import type { Commit } from "@gitto/git/types";

import { avatarColor, initials, relativeTime } from "@/lib/format";

import { layOutGraph, type GraphRow, type GraphState } from "./graph";
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

/** What the history table shows: a repository's log, below its uncommitted changes if any. */
export interface HistoryInput {
  repositoryId: string;
  /** Newest first. */
  commits: Commit[];
  hasChanges: boolean;
  /** The commit the uncommitted changes are based on, which they get a dashed line to. */
  head: string | undefined;
}

/** The history table's rows, with what it takes to add to them when more commits are loaded. */
export interface History extends HistoryInput {
  rows: HistoryRow[];
  /** Whether `head` is among the commits. */
  headShown: boolean;
  /** Where the graph's layout ended. */
  graph: GraphState;
}

/**
 * Turns a repository's log, plus the uncommitted changes if there are any, into history table
 * rows, each with its row of the graph. When `previous` is the same history with fewer commits
 * loaded, only the new ones are turned into rows and laid out.
 */
export function buildHistory(input: HistoryInput, previous?: History): History {
  const { repositoryId, commits, hasChanges, head } = input;
  const added = previous && addedCommits(previous, input);
  if (previous && added) {
    const graph = layOutGraph(added, previous.graph);
    const rows = previous.rows.concat(
      added.map((commit, index) => toCommitRow(repositoryId, commit, graph.rows[index])),
    );
    return { ...input, rows, headShown: previous.headShown, graph: graph.state };
  }

  const headShown = head !== undefined && commits.some((commit) => commit.sha === head);
  const wip = hasChanges ? [{ sha: WIP_ID, parents: headShown ? [head] : [], dashed: true }] : [];
  const graph = layOutGraph([...wip, ...commits]);
  const rows: HistoryRow[] = commits.map((commit, index) =>
    toCommitRow(repositoryId, commit, graph.rows[index + wip.length]),
  );
  if (hasChanges) rows.unshift({ kind: "wip", id: WIP_ID, repositoryId, graph: graph.rows[0] });
  return { ...input, rows, headShown, graph: graph.state };
}

/** The commits `next` has after `previous`' ones, if that's all that differs between them. */
function addedCommits(previous: History, next: HistoryInput): Commit[] | undefined {
  const count = previous.commits.length;
  if (
    previous.repositoryId !== next.repositoryId ||
    previous.hasChanges !== next.hasChanges ||
    previous.head !== next.head ||
    count === 0 ||
    next.commits.length <= count ||
    // The same commits to begin with: checking both ends is enough, as they're only ever replaced
    // all at once.
    next.commits[0] !== previous.commits[0] ||
    next.commits[count - 1] !== previous.commits[count - 1]
  ) {
    return undefined;
  }
  const added = next.commits.slice(count);
  // The uncommitted changes' line would have to run down to it, through the rows there already.
  const reachesHead =
    next.hasChanges && !previous.headShown && added.some((commit) => commit.sha === next.head);
  return reachesHead ? undefined : added;
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
