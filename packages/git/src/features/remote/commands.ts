import { readFile } from "node:fs/promises";
import { join } from "node:path";

import { GitError, NoUpstreamError, RepositoryChangedError } from "../../core/errors";
import { gitDirs, type GitDirs } from "../../core/git-dirs";
import { currentBranch, refExists, resolveRef, type GitCommand, type Repo } from "../../core/repo";
import {
  FALSE,
  makesMergeCommit,
  readConfig,
  rebases,
  rebaseSetting,
  remotesToFetch,
  type PullConfig,
} from "./config";
import { inProgress, interruption, progress, rebaseState } from "./progress";
import { NO_BRANCH, noUpstream, REBASING } from "./pull-blocker";

/**
 * Fetches every remote, and drops remote-tracking branches whose branch was deleted on the remote,
 * so the sidebar and history show the remotes as they are.
 */
export async function fetchAll(repo: Repo): Promise<void> {
  await gitFetch(repo.fetch, ["--all", "--prune", "--no-progress"]);
}

/**
 * Fetches `remotes` and prunes, as `fetchAll` does, once a pull has merged: not waited for, so a
 * remote that's slow to answer doesn't hold up the pull, and one that can't be reached doesn't fail
 * it (that's for the Fetch button to explain). The watcher picks up the branches it changes.
 */
function fetchAfterPull(repo: Repo, remotes: string[]): void {
  if (remotes.length === 0) return;
  // Queued (as fetches are) before the pull returns, so a fetch started after it waits for this.
  gitFetch(repo.fetch, ["--multiple", "--prune", "--no-progress", "--", ...remotes]).catch(
    () => undefined,
  );
}

/** `git fetch` with `args`, through `run` (`repo.fetch`, or a command of `repo.fetching`). */
async function gitFetch(run: GitCommand, args: string[], settings: string[] = []): Promise<void> {
  try {
    // Without the upkeep a fetch may start (gc, maintenance, the commit-graph): fetches run
    // alongside the queued writes, which that could get in the way of.
    await run(["fetch", ...args], {
      config: ["gc.auto=0", "maintenance.auto=false", "fetch.writeCommitGraph=false", ...settings],
    });
  } catch (error) {
    throw error instanceof GitError ? error.withoutHints() : error;
  }
}

/**
 * Fetches the current branch's upstream and merges it in, or rebases onto it if the user's config
 * says to, like `git pull`. Conflicts are left in the working tree, to resolve and commit. Also
 * fetches every remote and drops branches deleted on them, as the Fetch button does, so the
 * sidebar and history are up to date after a pull too: the upstream's remote first, and the others
 * once merged (see `fetchAfterPull`).
 *
 * Done as a fetch and then a merge or rebase, rather than one `git pull`, so only the second waits
 * for (and holds up) the other writes to the repository: the fetch takes as long as the network.
 */
export async function pull(repo: Repo): Promise<void> {
  // Where git keeps FETCH_HEAD and the state of a rebase; asked for once, and only awaited once
  // the branch is read, which can fail first: that's the error to report.
  const dirs = lazily(() => gitDirs(repo));
  const { branch, upstream, config: settings } = await readBranch(repo.read, dirs);
  const { gitDir } = await dirs();
  // Its merge commit is worded unless the pull will rebase, or only fast-forward, which makes none
  // (as the settings say before fetching).
  const fetched = await fetchUpstream(repo, gitDir, upstream, makesMergeCommit(settings, branch));

  try {
    await integrate(repo, gitDir, branch, upstream, fetched);
  } finally {
    // Also after a merge that stopped at conflicts, say: the remotes are fetched all the same.
    fetchAfterPull(repo, remotesToFetch(settings, upstream.remote));
  }
}

/** `read`, run the first time the result is asked for, and once. */
function lazily<T>(read: () => Promise<T>): () => Promise<T> {
  let result: Promise<T> | undefined;
  return () => (result ??= read());
}

/**
 * Merges what `fetchUpstream` fetched into `branch`, or rebases onto it, as one write; rejects if
 * that stopped partway, or the branch or its upstream changed while fetching.
 */
async function integrate(
  repo: Repo,
  gitDir: string,
  branch: string,
  upstream: Upstream,
  fetched: Fetched | null,
): Promise<void> {
  await repo.exclusive(async (run) => {
    // The branch and its settings again, now that no other write can change them: one may have
    // while fetching. All that's read while holding up the other writes is read at once.
    const [current, config, before, hasHead] = await Promise.all([
      currentBranch(run),
      readConfig(run),
      progress(run, gitDir),
      refExists(run, "HEAD"),
    ]);
    if (current !== branch) {
      throw new RepositoryChangedError(`Switched from ${branch} while pulling it. Pull again.`);
    }
    if (
      config.get(`branch.${branch}.remote`) !== upstream.remote ||
      config.get(`branch.${branch}.merge`) !== upstream.merge
    ) {
      throw new RepositoryChangedError(
        `${branch}'s upstream changed while pulling it. Pull again.`,
      );
    }
    const tracking = upstream.tracking ?? (await trackingBranch(run, branch));
    if (!tracking) throw new NoUpstreamError(noUpstream(branch));
    const args = integrateArgs(config, branch, tracking, fetched, hasHead);

    let failure: GitError | undefined;
    try {
      // The merge or rebase rewrites the files the upstream changed.
      await run(args, { rewritesFiles: true });
    } catch (error) {
      if (!(error instanceof GitError)) throw error;
      failure = error;
    }
    // Git won't merge or rebase over a merge, rebase or conflicts that were already there, and
    // says so; those aren't the pull's to explain.
    if (!inProgress(before)) {
      const interrupted = await interruption(run, gitDir, args, tracking, failure);
      if (interrupted) throw interrupted;
    }
    if (failure) throw failure.withoutHints();
  });
}

interface Upstream {
  /** The remote's name; "." for a local branch. */
  remote: string;
  /** The branch's full name on the remote, e.g. `refs/heads/main`. */
  merge: string;
  /**
   * Its remote-tracking branch, e.g. `refs/remotes/origin/main`, or the local branch it is;
   * `null` until the branch has a commit (see `trackingBranch`).
   */
  tracking: string | null;
}

/** The checked-out branch, and the upstream it pulls from; rejects if there isn't one. */
async function readBranch(
  run: GitCommand,
  dirs: () => Promise<GitDirs>,
): Promise<{ branch: string; upstream: Upstream; config: PullConfig }> {
  const [branch, config] = await Promise.all([currentBranch(run), readConfig(run)]);
  if (branch === null) {
    // HEAD is detached while a rebase stops: it's that to finish, not a branch to check out.
    const rebasing = (await rebaseState((await dirs()).gitDir)).rebasing;
    throw new NoUpstreamError(rebasing ? REBASING : NO_BRANCH);
  }
  // From the config: a branch without commits yet has no ref to ask for its upstream. A remote
  // given as a URL, rather than one with a name (and so a URL setting), has no remote-tracking
  // branches.
  const remote = config.get(`branch.${branch}.remote`);
  const merge = config.get(`branch.${branch}.merge`);
  if (!remote || !merge || (remote !== "." && !config.has(`remote.${remote}.url`))) {
    throw new NoUpstreamError(noUpstream(branch));
  }
  // `git pull` merges them all at once; only one is merged here.
  if (config.all(`branch.${branch}.merge`).length > 1) {
    throw new NoUpstreamError(
      `${branch} tracks several branches, which only git pull, in a terminal, merges at once.`,
    );
  }
  // The remote-tracking branch it's fetched into, known (once the branch has a commit to be
  // listed) even before the first fetch. Without one, as for an upstream given as a URL,
  // `git status` reports no upstream either, so the UI doesn't offer to pull: refused before
  // fetching.
  // Asked for by its full name, but `for-each-ref` also lists the refs under it (`main/x`, when
  // `main` has no commits): the branch's own line is picked out.
  const ref = `refs/heads/${branch}`;
  const listed = await run(["for-each-ref", "--format=%(refname)%00%(upstream)", ref]);
  const line = listed.split("\n").find((entry) => entry.startsWith(`${ref}\0`));
  const tracking = line === undefined ? null : line.slice(ref.length + 1);
  if (tracking === "") throw new NoUpstreamError(noUpstream(branch));
  return { branch, upstream: { remote, merge, tracking }, config };
}

/** What a pull fetched, to merge. */
interface Fetched {
  /** The upstream's commit, as it was fetched. */
  sha: string;
  /** The merge commit's message, worded by git as `git pull` would. */
  message: string;
}

/**
 * Fetches the branch's upstream as `git pull` does: its whole remote, which updates the
 * remote-tracking branches, and FETCH_HEAD, read before another fetch replaces it. Pruned too, as
 * the Fetch button does, so the remote is only asked once. A local upstream (remote ".") has
 * nothing to fetch. With what was fetched and the message to merge it with, if `word`, and there's
 * one: there's none for a branch already up to date, or without commits (which just moves to its
 * upstream). Rejects if the upstream no longer exists.
 */
async function fetchUpstream(
  repo: Repo,
  gitDir: string,
  { remote, merge, tracking }: Upstream,
  word: boolean,
): Promise<Fetched | null> {
  // The upstream's commit, and its line of FETCH_HEAD: the one to merge, and the only one.
  let sha: string;
  let fetchHead: string;
  if (remote === ".") {
    if (!tracking) return null;
    const local = await resolveRef(repo.read, tracking);
    if (!local) {
      throw new NoUpstreamError(`${tracking.replace(/^refs\/heads\//, "")} no longer exists.`);
    }
    sha = local;
    // As `git pull` words FETCH_HEAD for a local branch.
    fetchHead = `${sha}\t\t${fetchedName(merge)} of .\n`;
  } else {
    const written = await repo.fetching(async (run) => {
      // FETCH_HEAD is read next, so it's written whatever `fetch.writeFetchHEAD` says.
      await gitFetch(run, ["--quiet", "--prune", "--", remote], ["fetch.writeFetchHEAD=true"]);
      return readFile(join(gitDir, "FETCH_HEAD"), "utf8");
    });
    // The upstream's line, picked by its name rather than by git marking it as the one to merge:
    // git marks the upstream of the branch checked out as it fetched, which may have been another.
    // None for an upstream deleted on the remote, whose remote-tracking branch was just pruned.
    const name = `${fetchedName(merge)} of `;
    const line = written.split("\n").find((entry) => entry.split("\t")[2]?.startsWith(name));
    if (!line) {
      throw new NoUpstreamError(
        `${merge.replace(/^refs\/heads\//, "")} no longer exists on ${remote}.`,
      );
    }
    const [fetchedSha = "", , description = ""] = line.split("\t");
    sha = fetchedSha;
    // Marked as the one to merge.
    fetchHead = `${sha}\t\t${description}\n`;
  }
  if (!sha || !word) return null;
  // Worded from FETCH_HEAD, as `git pull` has it: e.g. "Merge branch 'main' of <URL>", the URL
  // as the fetch wrote it there (without credentials). Nothing to word for a branch already up
  // to date, or without commits.
  const message = await repo.read(["fmt-merge-msg"], { stdin: fetchHead }).then(
    (output) => output.trim(),
    () => "",
  );
  return message ? { sha, message } : null;
}

/** The kinds of refs `git fetch` names in FETCH_HEAD, by the prefix of their full names. */
const FETCHED_KINDS: [prefix: string, kind: string][] = [
  ["refs/heads/", "branch"],
  ["refs/tags/", "tag"],
  ["refs/remotes/", "remote-tracking branch"],
];

/** How `git fetch` names `ref` in FETCH_HEAD, before " of <URL>": e.g. "branch 'main'". */
function fetchedName(ref: string): string {
  for (const [prefix, kind] of FETCHED_KINDS) {
    if (ref.startsWith(prefix)) return `${kind} '${ref.slice(prefix.length)}'`;
  }
  return `'${ref}'`;
}

/**
 * The remote-tracking branch `branch`'s upstream was fetched into, for a branch without commits,
 * which `for-each-ref` doesn't list: it's only found once that exists, after the fetch. `null`
 * without one.
 */
function trackingBranch(run: GitCommand, branch: string): Promise<string | null> {
  return run(["rev-parse", "--symbolic-full-name", `${branch}@{upstream}`]).then(
    (ref) => ref.trim() || null,
    (error: unknown) => {
      // Git says there's no upstream, or none fetched; anything else (no longer a repository,
      // say, a GitError of its own) is for the caller.
      if (
        error instanceof GitError &&
        error.constructor === GitError &&
        /upstream|remote-tracking|unknown revision/.test(error.stderr)
      ) {
        return null;
      }
      throw error;
    },
  );
}

/**
 * The merge or rebase `git pull` would do after fetching, going by the same settings. Unlike
 * `git pull`, which refuses to pull a branch that diverged from its upstream until told whether
 * to merge or rebase, this merges then, like git did by default before 2.27. And it never rebases
 * interactively, which would open an editor with no one to use it.
 */
function integrateArgs(
  config: PullConfig,
  branch: string,
  tracking: string,
  fetched: Fetched | null,
  hasHead: boolean,
): string[] {
  const ff = config.get("pull.ff")?.toLowerCase();
  // A branch without commits yet has nothing to rebase: it's merged, which just moves it there.
  if (hasHead && rebases(config, branch)) {
    const rebase = rebaseSetting(config, branch);
    return [
      "rebase",
      "--quiet",
      ...(rebase === "merges" || rebase === "m" ? ["--rebase-merges"] : []),
      // From where the branch forked from its upstream, like `git pull --rebase`.
      "--fork-point",
      tracking,
    ];
  }
  const ffArgs =
    !hasHead || ff === undefined
      ? []
      : ff === "only"
        ? ["--ff-only"]
        : FALSE.has(ff)
          ? ["--no-ff"]
          : ["--ff"];
  // What was fetched, with the message `git pull` would give it; or, without one, the
  // remote-tracking branch.
  const target = fetched ? ["-m", fetched.message, fetched.sha] : [tracking];
  // Its log, if `merge.log` wants one, is already in the message, and its comments are git's own.
  const wording = fetched ? ["--no-log", "--cleanup=strip"] : [];
  return ["merge", "--quiet", "--no-edit", ...wording, ...ffArgs, ...target];
}
