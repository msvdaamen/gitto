// Reading the history from what changed since it was last read, rather than all over again: in a
// big repository, sorting the history of every branch and tag takes long (0.4s for vscode's 5,800),
// while what a commit, a pull or a fetch adds is found in a fraction of that.
import type { Repo } from "../../core/repo";
import { CommitIndex } from "./commit-index";
import { LOG_FORMAT, parseDecorations, parseLog } from "./parse";
import type { Commit, CommitRef } from "./schema";

export type Page = { limit: number; skip: number };

/**
 * What the log starts from, besides HEAD: every branch, remote branch and tag (and the replace
 * refs, which change what a commit's parents are), with what a tag points at, and the checked-out
 * branch marked, as both are in the log's decorations.
 */
export const LOG_REFS_ARGS = [
  "for-each-ref",
  "--format=%(objectname)%00%(objecttype)%00%(*objectname)%00%(*objecttype)%00%(HEAD)%00%(refname)",
  "refs/heads",
  "refs/remotes",
  "refs/tags",
  "refs/replace",
];

interface LogRef {
  /** What it points at; a tag object for an annotated tag. */
  sha: string;
  /**
   * The commit it starts the log from and decorates: for an annotated tag, the one the tag points
   * at. `null` for a tag of something else, like a tree, and `undefined` for a tag of a tag, which
   * would take asking git again to tell.
   */
  commit: string | null | undefined;
  /** Whether it's the checked-out branch. */
  current: boolean;
}

/** `LOG_REFS_ARGS` output, by ref name. */
export function parseLogRefs(output: string): Map<string, LogRef> {
  const refs = new Map<string, LogRef>();
  for (const line of output.split("\n")) {
    const [sha = "", type, peeled = "", peeledType, head, name = ""] = line.split("\0");
    if (!name) continue;
    const commit =
      type === "commit"
        ? sha
        : type !== "tag"
          ? null
          : peeledType === "commit"
            ? peeled
            : peeledType === "tag"
              ? undefined
              : null;
    refs.set(name, { sha, commit, current: head === "*" });
  }
  return refs;
}

/**
 * The commits the log starts from, for the commit index; `undefined` if one of them can't be told
 * (see `LogRef.commit`).
 */
function indexTips(refs: Map<string, LogRef>, head: string | null): Set<string> | undefined {
  const shas = new Set<string>();
  for (const [name, ref] of refs) {
    if (name.startsWith("refs/replace/")) continue;
    if (ref.commit === undefined) return undefined;
    if (ref.commit) shas.add(ref.commit);
  }
  if (head) shas.add(head);
  return shas;
}

function sameSet(a: ReadonlySet<string>, b: ReadonlySet<string>): boolean {
  return a.size === b.size && [...a].every((item) => b.has(item));
}

/** A page of the log, and what it was read from. */
export interface LogSnapshot {
  version: string;
  page: Page;
  /** HEAD's commit; `null` before the first commit. */
  head: string | null;
  refs: Map<string, LogRef>;
  /** The page, and the commit after it (if there is one), to tell what's past the page. */
  commits: Commit[];
}

interface LogMemory {
  /** The pages read last, newest last: the caller has one of these, usually the newest. */
  snapshots: LogSnapshot[];
  /** How long reading a whole page took the last time it was. */
  fullReadMs: number;
  /** Once loaded: the commits the history reaches, as of the newest page, or one before it. */
  index?: CommitIndex;
  /** Loading the index, or bringing it up to date, in the background. */
  indexing?: Promise<void>;
  /** Not to be indexed: a shallow clone, whose commits' parents change as it's deepened. */
  unindexable?: boolean;
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
 * Loads the commit index in the background, if it isn't yet, or brings it up to date with
 * `snapshot`, which was read whole: reading the next page from what changed needs the index as of
 * the page it's read from.
 */
export function indexLog(repo: Repo, snapshot: LogSnapshot): void {
  const memory = memoryOf(repo);
  const wanted = indexTips(snapshot.refs, snapshot.head);
  if (memory.unindexable || memory.indexing || !wanted) return;
  if (memory.index && sameSet(memory.index.tips, wanted)) return;
  const work = async () => {
    if (memory.index) {
      await memory.index.sync(repo, wanted);
      return;
    }
    const shallow = await repo.read(["rev-parse", "--is-shallow-repository"]);
    if (shallow.trim() === "true") {
      memory.unindexable = true;
      return;
    }
    memory.index = await CommitIndex.load(repo, wanted);
  };
  memory.indexing = work()
    .catch(() => {
      // Left out of the next reads, rather than relied on half done; loaded again after the next.
      memory.index = undefined;
    })
    .finally(() => (memory.indexing = undefined));
}

/** Resolves once the commit index is done loading or being brought up to date in the background. */
export async function indexed(repo: Repo): Promise<CommitIndex | undefined> {
  const memory = memoryOf(repo);
  await memory.indexing;
  return memory.index;
}

/**
 * The first page of the log now, and the commit after it, read from what changed since `previous`:
 * the commits that are new, the ones before them in `previous`, and the decorations of those whose
 * refs moved. `undefined` when that wouldn't be the page git sorts itself; it then has to be read
 * whole.
 *
 * Git sorts the log by commit date, never a commit before one of its children: it repeatedly takes
 * the newest commit whose children have all been taken. New commits are only ever children of the
 * ones before (or of each other), so when they're all newer than any commit git could have taken
 * first before, they come first, in their own order, and the rest as before. Where git would choose
 * between commits made in the same second, it goes by the order it came across them in, which
 * isn't worked out here: those, and commits lost (a reset, a deleted branch), are read whole.
 *
 * Which commits are new and whether any were lost comes from the commit index, which needs to be as
 * of `previous`: git's own answer can be wrong when the clock was skewed (see `CommitIndex`).
 */
export async function readLogSince(
  repo: Repo,
  previous: LogSnapshot,
  head: string | null,
  refs: Map<string, LogRef>,
  signal?: AbortSignal,
): Promise<Commit[] | undefined> {
  const memory = memoryOf(repo);
  const { page } = previous;
  const commitIndex = memory.index;
  const [tipsBefore, tipsNow] = [indexTips(previous.refs, previous.head), indexTips(refs, head)];
  if (!commitIndex || memory.indexing || !tipsBefore || !tipsNow) return undefined;
  if (!sameSet(commitIndex.tips, tipsBefore)) return undefined;
  if (page.skip !== 0 || previous.commits.length === 0) return undefined;
  // The page, and the commit after it.
  const size = page.limit + 1;

  // Commits reachable from these may be new.
  const added = new Set<string>();
  // Commits whose decorations may have changed.
  const redecorated = new Set<string>();
  const touch = (sha: string | null | undefined) => sha && redecorated.add(sha);
  for (const [name, ref] of refs) {
    const was = previous.refs.get(name);
    if (was?.sha === ref.sha && was.current === ref.current) continue;
    if (name.startsWith("refs/replace/")) return replaced(memory);
    touch(ref.commit);
    touch(was?.commit);
    if (was?.sha !== ref.sha) added.add(ref.sha);
  }
  for (const [name, was] of previous.refs) {
    if (refs.has(name)) continue;
    if (name.startsWith("refs/replace/")) return replaced(memory);
    touch(was.commit);
  }
  if (head !== previous.head) {
    if (head) added.add(head);
    touch(head);
    touch(previous.head);
  }

  const inPage = new Map(previous.commits.map((commit, index) => [commit.sha, index]));
  // Revisions go in through stdin: there can be thousands.
  const read = (args: string[], revisions?: string[]) =>
    repo.read(args, { signal, stdin: revisions && `${revisions.join("\n")}\n` });
  const toRedecorate = [...redecorated].filter((sha) => inPage.has(sha));
  const [changes, newOutput, decorationsOutput] = await Promise.all([
    commitIndex.sync(repo, tipsNow, signal),
    // The new commits' details, along with (when the clock was skewed) some that aren't new.
    added.size
      ? read(
          ["log", "-z", LOG_FORMAT, "--decorate=full", `--max-count=${page.limit}`, "--stdin"],
          [...added, ...not(tips(previous.refs, previous.head))],
        )
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
  if (!changes || changes.lost) return undefined;

  // Commits of the same second are taken in the order git came across them, which goes by the refs
  // pointing at them: one whose refs changed can come up in another place than it did.
  const sameSecond = new Map<number, number>();
  for (const commit of previous.commits) {
    sameSecond.set(commit.committedAt, (sameSecond.get(commit.committedAt) ?? 0) + 1);
  }
  // Commits past the page of the same second as one on it are of the same second as the commit
  // after it too, which is kept for that: dates only go down the log.
  const tied = (sha: string) => {
    const index = inPage.get(sha);
    return index !== undefined && sameSecond.get(previous.commits[index]!.committedAt)! > 1;
  };
  if (toRedecorate.some(tied)) return undefined;

  const fresh = parseLog(newOutput).filter((commit) => changes.added.has(commit.sha));
  // As many as a page, or more (or some not read, past the ones that weren't new): reading it
  // whole is no slower.
  if (fresh.length >= page.limit || fresh.length < changes.added.size) return undefined;
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
  const kept = previous.commits.slice(0, size - ordered.length);
  for (const [i, commit] of kept.entries()) {
    const refsNow = decorations.get(commit.sha);
    // A copy: the pages kept before share their commits.
    if (refsNow) kept[i] = { ...commit, refs: refsNow };
  }
  return [...ordered, ...kept];
}

/** A replace ref changed what commits' parents are: the index is loaded again, later. */
function replaced(memory: LogMemory): undefined {
  memory.index = undefined;
  return undefined;
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
