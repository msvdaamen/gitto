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
 * refs, which change what a commit's parents are), with the checked-out branch marked, as it is in
 * the log's decorations.
 */
export const LOG_REFS_ARGS = [
  "for-each-ref",
  "--format=%(objectname)%00%(HEAD)%00%(refname)",
  "refs/heads",
  "refs/remotes",
  "refs/tags",
  "refs/replace",
];

interface LogRef {
  /** What it points at: a commit, or a tag object for an annotated tag. */
  sha: string;
  /** Whether it's the checked-out branch. */
  current: boolean;
}

/** `LOG_REFS_ARGS` output, by ref name. */
export function parseLogRefs(output: string): Map<string, LogRef> {
  const refs = new Map<string, LogRef>();
  for (const line of output.split("\n")) {
    const [sha = "", head, name = ""] = line.split("\0");
    if (name) refs.set(name, { sha, current: head === "*" });
  }
  return refs;
}

/**
 * Finds out what the objects `refs` point at end at (see `LogMemory.refCommits`), for those it
 * isn't known of: asking git for every ref's along with the refs (`%(*objectname)`) would make
 * reading them five times slower in a big repository, every time.
 */
async function peel(repo: Repo, refs: Map<string, LogRef>[], signal?: AbortSignal) {
  const { refCommits } = memoryOf(repo);
  const unknown = new Set<string>();
  for (const named of refs) {
    for (const [name, ref] of named) {
      if (!name.startsWith("refs/replace/") && !refCommits.has(ref.sha)) unknown.add(ref.sha);
    }
  }
  if (unknown.size === 0) return;
  const check = async (objects: string[]) => {
    const output = await repo.read(["cat-file", "--batch-check=%(objectname) %(objecttype)"], {
      signal,
      stdin: `${objects.join("\n")}\n`,
    });
    return output
      .split("\n")
      .slice(0, objects.length)
      .map((line) => line.split(" "));
  };
  // Telling an object's type is quick; peeling a tag takes reading it, so only tags are.
  const shas = [...unknown];
  const tags: string[] = [];
  for (const [i, [, type]] of (await check(shas)).entries()) {
    if (type === "tag") tags.push(shas[i]!);
    else refCommits.set(shas[i]!, type === "commit" ? shas[i]! : null);
  }
  if (tags.length === 0) return;
  for (const [i, [sha, type]] of (await check(tags.map((tag) => `${tag}^{}`))).entries()) {
    refCommits.set(tags[i]!, type === "commit" ? sha! : null);
  }
}

/**
 * The commits the log starts from, for the commit index; `undefined` while what a ref points at
 * isn't known (see `peel`).
 */
function indexTips(
  memory: LogMemory,
  refs: Map<string, LogRef>,
  head: string | null,
): Set<string> | undefined {
  const shas = new Set<string>();
  for (const [name, ref] of refs) {
    if (name.startsWith("refs/replace/")) continue;
    const commit = memory.refCommits.get(ref.sha);
    if (commit === undefined) return undefined;
    if (commit) shas.add(commit);
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
  /**
   * The commit each object refs point at starts the log from, by the object's SHA: the commit
   * itself, the one an annotated tag points at (through any tags in between), or `null` for
   * anything else, like a tag of a tree. Kept, as an object never changes.
   */
  refCommits: Map<string, string | null>;
}

/** How many pages are kept, in case the caller didn't get the newest (it was cancelled, say). */
const SNAPSHOTS = 3;

function memoryOf(repo: Repo): LogMemory {
  let memory = repo.memory.get("log") as LogMemory | undefined;
  if (!memory)
    repo.memory.set("log", (memory = { snapshots: [], fullReadMs: 0, refCommits: new Map() }));
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
  if (memory.unindexable || memory.indexing) return;
  const known = indexTips(memory, snapshot.refs, snapshot.head);
  if (known && memory.index && sameSet(memory.index.tips, known)) return;
  const work = async () => {
    await peel(repo, [snapshot.refs]);
    const wanted = indexTips(memory, snapshot.refs, snapshot.head)!;
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
  if (!commitIndex || memory.indexing) return undefined;
  if (page.skip !== 0 || previous.commits.length === 0) return undefined;
  // Known for the refs before, but for those that moved, usually.
  await peel(repo, [previous.refs, refs], signal);
  const commitOf = (ref: LogRef | undefined) => ref && memory.refCommits.get(ref.sha);
  const [tipsBefore, tipsNow] = [
    indexTips(memory, previous.refs, previous.head)!,
    indexTips(memory, refs, head)!,
  ];
  if (!sameSet(commitIndex.tips, tipsBefore)) return undefined;
  // The page, and the commit after it.
  const size = page.limit + 1;

  // Commits reachable from these may be new: not from those reachable before, as neither are their
  // parents (a branch moved back, say).
  const added = new Set<string>();
  const add = (sha: string | null | undefined) => sha && !commitIndex.has(sha) && added.add(sha);
  // Commits whose decorations may have changed.
  const redecorated = new Set<string>();
  const touch = (sha: string | null | undefined) => sha && redecorated.add(sha);
  for (const [name, ref] of refs) {
    const was = previous.refs.get(name);
    if (was?.sha === ref.sha && was.current === ref.current) continue;
    if (name.startsWith("refs/replace/")) return replaced(memory);
    touch(commitOf(ref));
    touch(commitOf(was));
    if (was?.sha !== ref.sha) add(commitOf(ref));
  }
  for (const [name, was] of previous.refs) {
    if (refs.has(name)) continue;
    if (name.startsWith("refs/replace/")) return replaced(memory);
    touch(commitOf(was));
  }
  if (head !== previous.head) {
    add(head);
    touch(head);
    touch(previous.head);
  }

  const inPage = new Map(previous.commits.map((commit, index) => [commit.sha, index]));
  // Revisions go in through stdin: there can be thousands.
  const read = (args: string[], revisions?: string[]) =>
    repo.read(args, { signal, stdin: revisions && `${revisions.join("\n")}\n` });
  const syncing = commitIndex.sync(repo, tipsNow, signal);
  const reading = Promise.all([
    // The new commits' details, along with (when the clock was skewed) some that aren't new.
    added.size
      ? read(
          ["log", "-z", LOG_FORMAT, "--decorate=full", `--max-count=${page.limit}`, "--stdin"],
          [...added, ...not(tips(previous.refs, previous.head))],
        )
      : "",
    // Their dates too, for those past the page.
    redecorated.size
      ? read(
          [
            "log",
            "-z",
            "--no-walk=unsorted",
            "--ignore-missing",
            "--decorate=full",
            "--format=%H%x00%ct%x00%D",
            "--stdin",
          ],
          [...redecorated],
        )
      : "",
  ]);
  // Not waited for when commits were lost: the page is read whole anyway.
  reading.catch(() => undefined);
  const changes = await syncing;
  if (!changes || changes.lost) return undefined;
  const [newOutput, redecoratedOutput] = await reading;

  const redecoratedNow = new Map<string, { committedAt: number; refs: CommitRef[] }>();
  const fields = redecoratedOutput.split("\0");
  for (let i = 0; i + 2 < fields.length; i += 3) {
    redecoratedNow.set(fields[i]!.trim(), {
      committedAt: Number(fields[i + 1]) * 1000,
      refs: parseDecorations(fields[i + 2]!),
    });
  }

  // Where git could take more than one commit of the same second, it takes them in the order it
  // came across them: the ones it starts from by the refs pointing at them, the others as their
  // last child is taken. A commit whose refs changed, or which has a new child, may then come up in
  // another place among those it was tied with, so the page is read whole.
  //
  // The commits git chose `sha` over, or the other way around, are among the ones read: those
  // before it of its second, and the ones after it but for its parents and their parents newer than
  // it (when the clock was skewed), up to one older than it. Without one older read, they can be
  // past the page. A commit past the page can only come up on it in place of one of its second.
  const full = previous.commits.length === size;
  const tied = (sha: string, committedAt: number) => {
    if (previous.commits.some((c) => c.sha !== sha && c.committedAt === committedAt)) return true;
    const index = inPage.get(sha);
    if (index === undefined || !full) return false;
    return !previous.commits.slice(index + 1).some((c) => c.committedAt < committedAt);
  };
  for (const sha of redecorated) {
    // New commits come first, whatever their refs.
    if (changes.added.has(sha)) continue;
    const committedAt = redecoratedNow.get(sha)?.committedAt;
    if (committedAt === undefined || tied(sha, committedAt)) return undefined;
  }

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
    // did among those of its second: past the page, it stays there.
    const isTied = (sha: string) => {
      const index = inPage.get(sha);
      return index !== undefined && tied(sha, previous.commits[index]!.committedAt);
    };
    if (fresh.some((commit) => commit.parents.some(isTied))) return undefined;
  }

  const kept = previous.commits.slice(0, size - ordered.length);
  for (const [i, commit] of kept.entries()) {
    const refsNow = redecoratedNow.get(commit.sha)?.refs;
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
