// Reading the history from what changed since it was last read, rather than all over again: in a
// big repository, sorting the history of every branch and tag takes long (0.4s for vscode's 5,800),
// while what a commit, a pull or a fetch adds is found in a fraction of that.
import type { Repo } from "../../core/repo";
import { LOG_FORMAT, parseDecorations, parseLog } from "./parse";
import type { Commit, CommitRef } from "./schema";

export type Page = { limit: number; skip: number };

/**
 * What the log starts from, besides HEAD: every branch, remote branch and tag (and the replace
 * refs, which change what a commit's parents are), with the commit a tag points at, and the
 * checked-out branch marked, as both are in the log's decorations.
 */
export const LOG_REFS_ARGS = [
  "for-each-ref",
  "--format=%(objectname)%00%(*objectname)%00%(HEAD)%00%(refname)",
  "refs/heads",
  "refs/remotes",
  "refs/tags",
  "refs/replace",
];

interface LogRef {
  /** What it points at; a tag object for an annotated tag. */
  sha: string;
  /** The commit it decorates: for an annotated tag, the one the tag points at. */
  commit: string;
  /** Whether it's the checked-out branch. */
  current: boolean;
}

/** `LOG_REFS_ARGS` output, by ref name. */
export function parseLogRefs(output: string): Map<string, LogRef> {
  const refs = new Map<string, LogRef>();
  for (const line of output.split("\n")) {
    const [sha = "", peeled = "", head = "", name = ""] = line.split("\0");
    if (name) refs.set(name, { sha, commit: peeled || sha, current: head === "*" });
  }
  return refs;
}

/** A page of the log, and what it was read from. */
export interface LogSnapshot {
  version: string;
  page: Page;
  /** HEAD's commit; `null` before the first commit. */
  head: string | null;
  refs: Map<string, LogRef>;
  commits: Commit[];
}

interface LogMemory {
  /** The pages read last, newest last: the caller has one of these, usually the newest. */
  snapshots: LogSnapshot[];
  /** How long reading a whole page took the last time it was. */
  fullReadMs: number;
}

/** How many pages are kept, in case the caller didn't get the newest (it was cancelled, say). */
const SNAPSHOTS = 3;

function memoryOf(repo: Repo): LogMemory {
  let memory = repo.memory.get("log") as LogMemory | undefined;
  if (!memory) repo.memory.set("log", (memory = { snapshots: [], fullReadMs: 0 }));
  return memory;
}

/** The page of the log with version `version`, if it was read lately. */
export function rememberedLog(repo: Repo, version: string | undefined): LogSnapshot | undefined {
  return memoryOf(repo).snapshots.find((snapshot) => snapshot.version === version);
}

/** Keeps `snapshot`, to read the next page from; with how long it took, if it was read whole. */
export function rememberLog(repo: Repo, snapshot: LogSnapshot, readMs?: number): void {
  const memory = memoryOf(repo);
  memory.snapshots = [...memory.snapshots.slice(1 - SNAPSHOTS), snapshot];
  if (readMs !== undefined) memory.fullReadMs = readMs;
}

/** How long reading the whole page last took, to tell whether it's worth reading what changed. */
export function fullReadMs(repo: Repo): number {
  return memoryOf(repo).fullReadMs;
}

/**
 * The first page of the log now, read from what changed since `previous`: the commits that are new,
 * the ones before them in `previous`, and the decorations of those whose refs moved. `undefined`
 * when that wouldn't be the page git sorts itself; it then has to be read whole.
 *
 * Git sorts the log by commit date, never a commit before one of its children: it repeatedly takes
 * the newest commit whose children have all been taken. New commits are only ever children of the
 * ones before (or of each other), so when they're all newer than any commit git could have taken
 * first before, they come first, in their own order, and the rest as before. Where git would choose
 * between commits made in the same second, it goes by the order it came across them in, which
 * isn't worked out here: those, and commits lost (a reset, a deleted branch), are read whole.
 *
 * Not yet exact when commit dates go back in time (a skewed clock): asked for what's reachable from
 * the new tips but not the old ones, git stops walking by date, and can list commits it had already
 * as new. Those would have to be told apart before this can be relied on.
 */
export async function readLogSince(
  repo: Repo,
  previous: LogSnapshot,
  head: string | null,
  refs: Map<string, LogRef>,
  signal?: AbortSignal,
): Promise<Commit[] | undefined> {
  const { page } = previous;
  if (page.skip !== 0 || previous.commits.length === 0) return undefined;

  // Commits reachable from these may be new, and those reachable from `gone` only, gone.
  const added = new Set<string>();
  const gone = new Set<string>();
  // Commits whose decorations may have changed.
  const redecorated = new Set<string>();
  for (const [name, ref] of refs) {
    const was = previous.refs.get(name);
    if (was?.sha === ref.sha && was.current === ref.current) continue;
    if (name.startsWith("refs/replace/")) return undefined;
    redecorated.add(ref.commit);
    if (was) redecorated.add(was.commit);
    if (was?.sha === ref.sha) continue;
    added.add(ref.sha);
    if (was) gone.add(was.sha);
  }
  for (const [name, was] of previous.refs) {
    if (refs.has(name)) continue;
    if (name.startsWith("refs/replace/")) return undefined;
    redecorated.add(was.commit);
    gone.add(was.sha);
  }
  if (head !== previous.head) {
    if (head) {
      added.add(head);
      redecorated.add(head);
    }
    if (previous.head) {
      gone.add(previous.head);
      redecorated.add(previous.head);
    }
  }

  const inPage = new Map(previous.commits.map((commit, index) => [commit.sha, index]));
  // Revisions go in through stdin: there can be thousands.
  const read = (args: string[], revisions?: string[]) =>
    repo.read(args, { signal, stdin: revisions && `${revisions.join("\n")}\n` });
  const toRedecorate = [...redecorated].filter((sha) => inPage.has(sha));
  const [newOutput, lostOutput, decorationsOutput] = await Promise.all([
    // What's reachable now but wasn't, in no particular order.
    added.size
      ? read(
          ["log", "-z", LOG_FORMAT, "--decorate=full", `--max-count=${page.limit}`, "--stdin"],
          [...added, ...not(tips(previous.refs, previous.head))],
        )
      : "",
    // Anything that was reachable, but isn't anymore.
    gone.size
      ? read(["rev-list", "--max-count=1", "--stdin"], [...gone, ...not(tips(refs, head))])
      : "",
    toRedecorate.length
      ? read([
          "log",
          "-z",
          "--no-walk=unsorted",
          "--decorate=full",
          "--format=%H%x00%D",
          ...toRedecorate,
        ])
      : "",
  ]);
  if (lostOutput.trim()) return undefined;

  // Commits of the same second are taken in the order git came across them, which goes by the refs
  // pointing at them: one whose refs changed can come up in another place than it did.
  const sameSecond = new Map<number, number>();
  for (const commit of previous.commits) {
    sameSecond.set(commit.committedAt, (sameSecond.get(commit.committedAt) ?? 0) + 1);
  }
  const tied = (sha: string) => {
    const index = inPage.get(sha);
    return index !== undefined && sameSecond.get(previous.commits[index]!.committedAt)! > 1;
  };
  if (toRedecorate.some(tied)) return undefined;

  const fresh = parseLog(newOutput);
  // As many as a page, or more: reading it whole is no slower.
  if (fresh.length >= page.limit) return undefined;
  const ordered = byDate(fresh);
  if (!ordered) return undefined;
  if (ordered.length > 0) {
    // Newer than the commit git took first before, so newer than any it could take before them.
    const newest = previous.commits[0]!.committedAt;
    if (ordered.some((commit) => commit.committedAt <= newest)) return undefined;
    // A commit git could take from the start, which now has a new child, comes up later than it
    // did; between commits of the same second, that changes the order git takes them in.
    if (fresh.some((commit) => commit.parents.some(tied))) return undefined;
  }

  const decorations = new Map<string, CommitRef[]>();
  const fields = decorationsOutput.split("\0");
  for (let i = 0; i + 1 < fields.length; i += 2) {
    decorations.set(fields[i]!.trim(), parseDecorations(fields[i + 1]!));
  }
  const kept = previous.commits.slice(0, page.limit - ordered.length);
  for (const [i, commit] of kept.entries()) {
    const refsNow = decorations.get(commit.sha);
    // A copy: the pages kept before share their commits.
    if (refsNow) kept[i] = { ...commit, refs: refsNow };
  }
  return [...ordered, ...kept];
}

/** `shas` as revisions to leave out. */
function not(shas: Iterable<string>): string[] {
  return [...shas].map((sha) => `^${sha}`);
}

/** The commits the log starts from: what `refs` (but for replace refs) and `head` point at. */
function tips(refs: Map<string, LogRef>, head: string | null): Set<string> {
  const shas = new Set<string>();
  for (const [name, ref] of refs) if (!name.startsWith("refs/replace/")) shas.add(ref.sha);
  if (head) shas.add(head);
  return shas;
}

/**
 * `commits` as git sorts them by date: the newest of those whose children (among `commits`) have
 * all been taken, again and again. `undefined` when two it could take next are of the same second:
 * git then goes by the order it came across them in.
 */
function byDate(commits: Commit[]): Commit[] | undefined {
  const bySha = new Map(commits.map((commit) => [commit.sha, commit]));
  const children = new Map<string, number>();
  for (const commit of commits) {
    for (const parent of commit.parents) {
      if (bySha.has(parent)) children.set(parent, (children.get(parent) ?? 0) + 1);
    }
  }
  const ready = commits.filter((commit) => !children.get(commit.sha));
  const sorted: Commit[] = [];
  while (ready.length > 0) {
    let newest = 0;
    for (let i = 1; i < ready.length; i++) {
      if (ready[i]!.committedAt > ready[newest]!.committedAt) newest = i;
    }
    const { committedAt } = ready[newest]!;
    if (ready.some((commit, i) => i !== newest && commit.committedAt === committedAt)) {
      return undefined;
    }
    const [next] = ready.splice(newest, 1);
    sorted.push(next!);
    for (const parent of next!.parents) {
      const left = children.get(parent);
      if (left === undefined) continue;
      children.set(parent, left - 1);
      if (left === 1) ready.push(bySha.get(parent)!);
    }
  }
  return sorted;
}
