import { readFile, stat } from "node:fs/promises";
import { join } from "node:path";

import {
  GitError,
  NoUpstreamError,
  PullInterruptedError,
  RepositoryChangedError,
} from "../../core/errors";
import { currentBranch, refExists, resolveRef, type GitCommand, type Repo } from "../../core/repo";
import { gitDirs, type GitDirs } from "../watch/git-dirs";
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

/**
 * The remotes `git fetch --all` fetches (those not set to be skipped), going by `config`, but for
 * `except`, which a pull already fetched.
 */
function remotesToFetch(config: Map<string, string>, except: string): string[] {
  const remotes: string[] = [];
  for (const key of config.keys()) {
    const name = /^remote\.(.+)\.url$/.exec(key)?.[1];
    if (name === undefined || name === except) continue;
    const skip = config.get(`remote.${name}.skipfetchall`)?.toLowerCase();
    if (skip === undefined || FALSE.has(skip)) remotes.push(name);
  }
  return remotes;
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
    throw error instanceof GitError ? withoutHints(error) : error;
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
  // Where git keeps FETCH_HEAD and the state of a rebase; asked for once.
  const dirs = gitDirs(repo);
  // Awaited only once the branch is read, which can fail first; that's the error to report.
  dirs.catch(() => undefined);
  const { branch, upstream, config: settings } = await readBranch(repo.read, dirs);
  const { gitDir } = await dirs;
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
    if (!before.conflicts && !before.merging && !before.rebasing && !before.applying) {
      // Even one that went through: putting back local changes it stashed can conflict, and
      // `branch.<name>.mergeOptions` can stop a merge before committing, or squash it: then
      // what was merged isn't in HEAD.
      const after = await progress(run, gitDir);
      // A merge that went through, or stopped at conflicts, without a merge to commit: squashed,
      // if what it merged isn't in HEAD.
      const squashing =
        args[0] === "merge" &&
        !after.merging &&
        (!failure || after.conflicts) &&
        !(await isAncestor(run, args.at(-1)!, "HEAD"));
      if (after.conflicts || after.merging || after.rebasing || squashing) {
        const name = tracking.replace(/^refs\/(remotes|heads)\//, "");
        throw interruptedError(name, { ...after, squashing }, failure);
      }
    }
    if (failure) throw withoutHints(failure);
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
  dirs: Promise<GitDirs>,
): Promise<{ branch: string; upstream: Upstream; config: Map<string, string> }> {
  const [branch, config] = await Promise.all([currentBranch(run), readConfig(run)]);
  if (branch === null) {
    // HEAD is detached while a rebase stops: it's that to finish, not a branch to check out.
    const rebasing = (await rebaseState((await dirs).gitDir)).rebasing;
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
  if (merge.includes("\n")) {
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
  let fetchHead: string;
  if (remote === ".") {
    if (!tracking) return null;
    // As `git pull` words FETCH_HEAD for a local branch.
    const sha = await resolveRef(repo.read, tracking);
    if (!sha) {
      throw new NoUpstreamError(`${tracking.replace(/^refs\/heads\//, "")} no longer exists.`);
    }
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
    const [sha, , description] = line.split("\t");
    // Marked as the one to merge, and the only one.
    fetchHead = `${sha}\t\t${description}\n`;
  }
  // The line of what's to be merged, the one that isn't marked not-for-merge.
  const line = fetchHead.split("\n").find((entry) => /^[0-9a-f]+\t\t/.test(entry));
  const sha = line?.slice(0, line.indexOf("\t"));
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
 * The settings a pull depends on, in one read, by key. Git lowercases the section and name, but
 * not a branch's name or a URL: e.g. `branch.Feature.merge`. The last value of a key wins, as in
 * git, but for a branch's `merge`, whose values are all kept.
 */
async function readConfig(run: GitCommand): Promise<Map<string, string>> {
  const pattern = String.raw`^(pull\.(rebase|ff)|branch\..+\.(remote|merge|rebase)|remote\..+\.(url|skipfetchall))$`;
  let output = "";
  try {
    output = await run(["config", "-z", "--get-regexp", pattern]);
  } catch (error) {
    // None of them is set.
    if (!(error instanceof GitError && error.exitCode === 1)) throw error;
  }
  const config = new Map<string, string>();
  for (const entry of output.split("\0")) {
    if (!entry) continue;
    const newline = entry.indexOf("\n");
    // A boolean set without a value (`[pull] rebase`) has no newline: it's true.
    if (newline === -1) config.set(entry, "true");
    else {
      const key = entry.slice(0, newline);
      const value = entry.slice(newline + 1);
      // The branches a branch merges are every value of its `merge`, kept on lines of their own.
      const previous = /^branch\..+\.merge$/.test(key) ? config.get(key) : undefined;
      config.set(key, previous === undefined ? value : `${previous}\n${value}`);
    }
  }
  return config;
}

const FALSE = new Set(["false", "no", "off", "0", ""]);

/** How `branch` is set to be rebased when pulled, lowercased; `undefined` when it isn't set. */
function rebaseSetting(config: Map<string, string>, branch: string): string | undefined {
  return (config.get(`branch.${branch}.rebase`) ?? config.get("pull.rebase"))?.toLowerCase();
}

/** Whether a pull of `branch` can make a merge commit, going by `config`. */
function makesMergeCommit(config: Map<string, string>, branch: string): boolean {
  return !rebases(config, branch) && config.get("pull.ff")?.toLowerCase() !== "only";
}

/** Whether a pull of `branch` rebases, going by `config`: as in `git pull`, `pull.ff=only` wins. */
function rebases(config: Map<string, string>, branch: string): boolean {
  const rebase = rebaseSetting(config, branch);
  return (
    config.get("pull.ff")?.toLowerCase() !== "only" && rebase !== undefined && !FALSE.has(rebase)
  );
}

/**
 * The merge or rebase `git pull` would do after fetching, going by the same settings. Unlike
 * `git pull`, which refuses to pull a branch that diverged from its upstream until told whether
 * to merge or rebase, this merges then, like git did by default before 2.27. And it never rebases
 * interactively, which would open an editor with no one to use it.
 */
function integrateArgs(
  config: Map<string, string>,
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

/** What a merge, rebase or `git am` left unfinished in a repository. */
interface Progress {
  conflicts: boolean;
  merging: boolean;
  rebasing: boolean;
  /** Applying patches with `git am`, which keeps its state where some rebases do. */
  applying: boolean;
}

async function hasConflicts(run: GitCommand): Promise<boolean> {
  return (await run(["ls-files", "--unmerged"])) !== "";
}

function exists(file: string): Promise<boolean> {
  return stat(file).then(
    () => true,
    () => false,
  );
}

/**
 * Whether a rebase, or `git am`, is stopped in the worktree whose git directory is `gitDir`: they
 * keep their state there meanwhile, `am` where some rebases do, with a file saying it's `am`'s.
 */
async function rebaseState(gitDir: string): Promise<{ rebasing: boolean; applying: boolean }> {
  const [rebaseMerge, rebaseApply, applying] = await Promise.all(
    ["rebase-merge", "rebase-apply", "rebase-apply/applying"].map((name) =>
      exists(join(gitDir, name)),
    ),
  );
  return { rebasing: !!rebaseMerge || (!!rebaseApply && !applying), applying: !!applying };
}

async function progress(run: GitCommand, gitDir: string): Promise<Progress> {
  const [conflicts, merging, state] = await Promise.all([
    hasConflicts(run),
    refExists(run, "MERGE_HEAD"),
    rebaseState(gitDir),
  ]);
  return { conflicts, merging, ...state };
}

/** Whether `ancestor` is in `rev`'s history; rejects if git couldn't tell. */
function isAncestor(run: GitCommand, ancestor: string, rev: string): Promise<boolean> {
  return run(["merge-base", "--is-ancestor", ancestor, rev]).then(
    () => true,
    (error: unknown) => {
      if (error instanceof GitError && error.exitCode === 1) return false;
      throw error;
    },
  );
}

/** Says where the pull stopped, and what to do to finish it. */
function interruptedError(
  upstream: string,
  { conflicts, merging, rebasing, squashing }: Progress & { squashing: boolean },
  failure: GitError | undefined,
): PullInterruptedError {
  // Why git stopped, when it wasn't at conflicts: e.g. an untracked file in the way of a commit
  // a rebase replays, or a hook that turned a merge commit down.
  const reason = failure && withoutHints(failure).message;
  const stopped = (what: string, next: string) =>
    reason ? `${what}:\n${reason}\nFix that, then ${next}.` : `${what}. ${capitalize(next)}.`;
  let message: string;
  if (rebasing && !conflicts) {
    message = stopped(
      `Rebasing onto ${upstream} stopped partway`,
      "continue the rebase, or abort it",
    );
  } else if (rebasing) {
    message = `Pulling ${upstream} caused conflicts. Resolve them, then continue the rebase.`;
  } else if (merging && !conflicts) {
    message = stopped(
      `Merging ${upstream} stopped before committing`,
      "commit the merge, or abort it",
    );
  } else if (merging) {
    message = `Pulling ${upstream} caused conflicts. Resolve them, then commit the merge.`;
  } else if (squashing && !conflicts) {
    message = `Squashed ${upstream} into the staged changes, without committing. Commit them to finish pulling.`;
  } else if (squashing) {
    message = `Pulling ${upstream} caused conflicts. Resolve them, then commit the squashed changes.`;
  } else {
    // The pull stashed local changes (rebase.autoStash, merge.autoStash) and they conflict.
    message = `Pulled ${upstream}, but your local changes conflict with it. Resolve the conflicts; your changes are also kept in the stash.`;
  }
  return new PullInterruptedError(message);
}

function capitalize(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

/**
 * `error` without git's hints, which suggest commands to type and so don't help in the app; of the
 * same class, so it reaches the renderer the same way.
 */
function withoutHints(error: GitError): GitError {
  const message = error.message
    .split("\n")
    .filter((line) => !line.startsWith("hint:"))
    .join("\n")
    .trim();
  if (!message || message === error.message) return error;
  const Class = error.constructor as typeof GitError;
  return new Class(message, error.args, error.exitCode, error.stderr);
}
