import { readFile, stat } from "node:fs/promises";
import { resolve } from "node:path";

import {
  GitError,
  NoUpstreamError,
  PullInterruptedError,
  RepositoryChangedError,
} from "../../core/errors";
import { refExists, type GitCommand, type Repo } from "../../core/repo";
import type { RunOptions } from "../../core/runner";
import { NO_BRANCH, noUpstream, REBASING } from "./pull-blocker";

/**
 * Fetches every remote, and drops remote-tracking branches whose branch was deleted on the remote,
 * so the sidebar and history show the remotes as they are.
 */
export async function fetchAll(repo: Repo): Promise<void> {
  await gitFetch(repo.fetch, ["--all", "--prune", "--no-progress"]);
}

/** `git fetch` with `args`, through `run` (`repo.fetch`, or a command of `repo.fetching`). */
async function gitFetch(
  run: (args: string[], options: RunOptions) => Promise<string>,
  args: string[],
): Promise<void> {
  try {
    // Without the upkeep a fetch may start (gc, maintenance, the commit-graph): fetches run
    // alongside the queued writes, which that could get in the way of.
    await run(["fetch", ...args], {
      config: ["gc.auto=0", "maintenance.auto=false", "fetch.writeCommitGraph=false"],
    });
  } catch (error) {
    throw error instanceof GitError ? withoutHints(error) : error;
  }
}

/**
 * Fetches the current branch's upstream and merges it in, or rebases onto it if the user's config
 * says to, like `git pull`. Conflicts are left in the working tree, to resolve and commit.
 *
 * Done as a fetch and then a merge or rebase, rather than one `git pull`, so only the second waits
 * for (and holds up) the other writes to the repository: the fetch takes as long as the network.
 */
export async function pull(repo: Repo): Promise<void> {
  const { branch, upstream } = await readBranch(repo.read, repo.path);
  const fetched = await fetchUpstream(repo, upstream);

  await repo.exclusive(async (run) => {
    // The branch and its settings again, now that no other write can change them: one may have
    // while fetching. All that's read while holding up the other writes is read at once.
    const [current, config, before, hasHead] = await Promise.all([
      currentBranch(run),
      readConfig(run),
      progress(run, repo.path),
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
      await run(args);
    } catch (error) {
      if (!(error instanceof GitError)) throw error;
      failure = error;
    }
    // Git won't merge or rebase over a merge, rebase or conflicts that were already there, and
    // says so; those aren't the pull's to explain.
    if (!before.conflicts && !before.merging && !before.rebasing && !before.applying) {
      // A merge or rebase that went through can only have left the conflicts of local changes
      // it stashed and put back, or a merge to commit (`branch.<name>.mergeOptions=--no-commit`).
      let after = before;
      if (failure) after = await progress(run, repo.path);
      else {
        const [conflicts, merging] = await Promise.all([
          hasConflicts(run),
          refExists(run, "MERGE_HEAD"),
        ]);
        after = { ...before, conflicts, merging };
      }
      if (after.conflicts || after.merging || after.rebasing) {
        const name = tracking.replace(/^refs\/(remotes|heads)\//, "");
        throw interruptedError(name, after, args, failure);
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
  path: string,
): Promise<{ branch: string; upstream: Upstream }> {
  const [branch, config] = await Promise.all([currentBranch(run), readConfig(run)]);
  if (branch === null) {
    // HEAD is detached while a rebase stops: it's that to finish, not a branch to check out.
    const { rebasing } = await progress(run, path);
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
  return { branch, upstream: { remote, merge, tracking } };
}

/** What a pull fetched, to merge. */
interface Fetched {
  /** The upstream's commit, as it was fetched. */
  sha: string;
  /** The merge commit's message, worded by git as `git pull` would; `null` if it had none. */
  message: string | null;
}

/**
 * Fetches the branch's upstream as `git pull` does: just that branch, which updates its
 * remote-tracking branch, and FETCH_HEAD, read before another fetch replaces it. A local upstream
 * (remote ".") has nothing to fetch. `null` when what was fetched isn't known yet: for a branch
 * without commits, which just moves to its upstream.
 */
async function fetchUpstream(
  repo: Repo,
  { remote, merge, tracking }: Upstream,
): Promise<Fetched | null> {
  let fetchHead: string;
  if (remote === ".") {
    if (!tracking) return null;
    // As `git pull` words FETCH_HEAD for a local branch.
    const sha = (await repo.read(["rev-parse", "--verify", `${tracking}^{commit}`])).trim();
    fetchHead = `${sha}\t\tbranch '${merge.replace(/^refs\/heads\//, "")}' of .\n`;
  } else {
    fetchHead = await repo.fetching(async (run) => {
      await gitFetch(run, ["--quiet", remote, merge]);
      const file = (await run(["rev-parse", "--git-path", "FETCH_HEAD"])).trim();
      return readFile(resolve(repo.path, file), "utf8");
    });
  }
  const sha = fetchHead.slice(0, fetchHead.indexOf("\t"));
  if (!sha) return null;
  // Worded from FETCH_HEAD, as `git pull` has it: e.g. "Merge branch 'main' of <URL>", the URL
  // as the fetch wrote it there (without credentials). Nothing to word for a branch already up
  // to date, or without commits.
  const message = await repo.read(["fmt-merge-msg"], { stdin: fetchHead }).then(
    (output) => output.trim() || null,
    () => null,
  );
  return { sha, message };
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
      // Git says there's no upstream, or none fetched; anything else is for the caller.
      if (
        error instanceof GitError &&
        /upstream|remote-tracking|unknown revision/.test(error.stderr)
      ) {
        return null;
      }
      throw error;
    },
  );
}

/** The checked-out branch's name; `null` when HEAD is detached. */
async function currentBranch(run: GitCommand): Promise<string | null> {
  let ref: string;
  try {
    // The full name: `--short` would make it `heads/main` if there's also a tag called `main`.
    ref = (await run(["symbolic-ref", "--quiet", "HEAD"])).trim();
  } catch (error) {
    if (error instanceof GitError && error.exitCode === 1) return null;
    throw error;
  }
  return ref.startsWith("refs/heads/") ? ref.slice("refs/heads/".length) : null;
}

/**
 * The settings a pull depends on, in one read, by key. Git lowercases the section and name, but
 * not a branch's name or a URL: e.g. `branch.Feature.merge`. The last value of a key wins, as in
 * git.
 */
async function readConfig(run: GitCommand): Promise<Map<string, string>> {
  const pattern = String.raw`^(pull\.(rebase|ff)|branch\..+\.(remote|merge|rebase)|remote\..+\.url)$`;
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
    else config.set(entry.slice(0, newline), entry.slice(newline + 1));
  }
  return config;
}

const FALSE = new Set(["false", "no", "off", "0", ""]);

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
  const rebase = (
    config.get(`branch.${branch}.rebase`) ?? config.get("pull.rebase")
  )?.toLowerCase();
  const ff = config.get("pull.ff")?.toLowerCase();
  // A branch without commits yet has nothing to rebase: it's merged, which just moves it there.
  // And as in `git pull`, `pull.ff=only` wins over rebasing.
  if (hasHead && ff !== "only" && rebase !== undefined && !FALSE.has(rebase)) {
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
  // What was fetched, with the message `git pull` would give it; or, before there's a commit to
  // word one against, the remote-tracking branch.
  const message = fetched?.message ? ["-m", fetched.message] : [];
  return ["merge", "--quiet", "--no-edit", ...ffArgs, ...message, fetched?.sha ?? tracking];
}

/** What a merge, rebase or `git am` left unfinished in the repository at `path`. */
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

async function progress(run: GitCommand, path: string): Promise<Progress> {
  const [conflicts, merging, dirs] = await Promise.all([
    hasConflicts(run),
    refExists(run, "MERGE_HEAD"),
    run([
      "rev-parse",
      ...["rebase-merge", "rebase-apply", "rebase-apply/applying"].flatMap((p) => [
        "--git-path",
        p,
      ]),
    ]),
  ]);
  // A rebase keeps its state in one of these while it's stopped, with or without conflicts; `am`
  // in the second, with a file saying so. Relative to the repository, or absolute, e.g. in a
  // linked worktree.
  const [rebaseMerge, rebaseApply, applying] = await Promise.all(
    dirs
      .split("\n")
      .filter(Boolean)
      .map((dir) =>
        stat(resolve(path, dir)).then(
          () => true,
          () => false,
        ),
      ),
  );
  return {
    conflicts,
    merging,
    rebasing: !!rebaseMerge || (!!rebaseApply && !applying),
    applying: !!applying,
  };
}

/** Says where the pull stopped, and what to do to finish it. */
function interruptedError(
  upstream: string,
  { conflicts, merging, rebasing }: Progress,
  args: string[],
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
  } else {
    // The pull stashed local changes (rebase.autoStash, merge.autoStash) and they conflict.
    message = `Pulled ${upstream}, but your local changes conflict with it. Resolve the conflicts; your changes are also kept in the stash.`;
  }
  return new PullInterruptedError(message, args, failure?.exitCode ?? 0, failure?.stderr ?? "");
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
