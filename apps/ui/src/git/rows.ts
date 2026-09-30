import type { Commit as GitCommit, Status } from "@gitto/git/types";

import type { Commit } from "@/types/git";

import { computeGraph } from "./graph";

/** Row id of the uncommitted changes, shown above the history when there are any. */
const WIP_ID = "wip";

const AVATAR_COLORS = ["#7c5ce7", "#38bda9", "#a978dd", "#e2a646", "#5b9be6", "#e0707a"];

/**
 * Turns a repository's log, plus the uncommitted changes if there are any, into history table
 * rows. Each row records the repository, so follow-up requests (like its changed files) always go
 * to the repository the commit is in.
 */
export function toCommitRows(
  repositoryId: string,
  log: GitCommit[],
  status: Status | undefined,
): Commit[] {
  const hasChanges = !!status && status.files.length > 0;
  const wip = { sha: WIP_ID, parents: status?.head ? [status.head] : [] };
  const graph = computeGraph(hasChanges ? [wip, ...log] : log);
  const offset = hasChanges ? 1 : 0;

  const rows = log.map<Commit>((commit, index) => ({
    repositoryId,
    id: commit.sha,
    sha: commit.sha.slice(0, 7),
    graph: graph[index + offset]!,
    message: commit.subject,
    description: commit.body || undefined,
    author: commit.authorName,
    initials: initials(commit.authorName),
    avatarColor: avatarColor(commit.authorEmail),
    timestamp: relativeTime(commit.authoredAt),
    refs: commit.refs,
  }));
  if (!hasChanges) return rows;

  return [
    {
      repositoryId,
      id: WIP_ID,
      sha: "working",
      // Hollow, as it isn't a commit (yet).
      graph: graph[0]!.map((cell) => (cell === "●" ? "○" : cell)),
      message: "Uncommitted changes",
      description: "Uncommitted changes in your working directory.",
      author: "You",
      initials: "YO",
      avatarColor: "#a978dd",
      timestamp: "Now",
      refs: ["WIP"],
      isWip: true,
    },
    ...rows,
  ];
}

function initials(name: string): string {
  const words = name.split(/[\s._-]+/).filter(Boolean);
  const letters = words.length > 1 ? words[0]![0]! + words.at(-1)![0]! : name.slice(0, 2);
  return letters.toUpperCase();
}

function avatarColor(email: string): string {
  let hash = 0;
  for (const char of email.toLowerCase()) hash = (hash * 31 + char.charCodeAt(0)) | 0;
  return AVATAR_COLORS[Math.abs(hash) % AVATAR_COLORS.length]!;
}

const relativeTimeFormat = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });

const TIME_UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["year", 365 * 24 * 60 * 60 * 1000],
  ["month", 30 * 24 * 60 * 60 * 1000],
  ["week", 7 * 24 * 60 * 60 * 1000],
  ["day", 24 * 60 * 60 * 1000],
  ["hour", 60 * 60 * 1000],
  ["minute", 60 * 1000],
];

function relativeTime(timestamp: number): string {
  const diff = timestamp - Date.now();
  for (const [unit, size] of TIME_UNITS) {
    if (Math.abs(diff) >= size) return relativeTimeFormat.format(Math.round(diff / size), unit);
  }
  return "Just now";
}
