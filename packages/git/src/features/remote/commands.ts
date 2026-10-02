import { stat } from "node:fs/promises";
import { resolve } from "node:path";

import { GitError, NoUpstreamError, PullInterruptedError } from "../../core/errors";
import { refExists, type GitCommand, type Repo } from "../../core/repo";
import { pullBlocker } from "./pull-blocker";

/**
 * Fetches the current branch's upstream and merges it in, or rebases onto it if the user's config
 * says to, like `git pull`. Conflicts are left in the working tree, to resolve and commit.
 *
 * Done as a fetch and then a merge or rebase, rather than one `git pull`, so only the second waits
 * for (and holds up) the other writes to the repository: the fetch takes as long as the network.
 */
export async function pull(repo: Repo): Promise<void> {
  const { branch, upstream } = await readBranch(repo.read);
  const target = await fetchUpstream(repo, branch, upstream);

  await repo.exclusive(async (run) => {
    // Read again, now that no other write can change them: one may have while fetching.
    const [current, before, hasHead] = await Promise.all([
      readBranch(run),
      progress(run, repo.path),
      refExists(run, "HEAD"),
    ]);
    if (current.branch !== branch) {
      throw new GitError(`Switched from ${branch} while pulling it. Pull again.`, [], null, "");
    }
    if (current.upstream.remote !== upstream.remote || current.upstream.merge !== upstream.merge) {
      throw new GitError(
        `${branch}'s upstream changed while pulling it. Pull again.`,
        [],
        null,
        "",
      );
    }
    const args = integrateArgs(current.upstream, branch, target, hasHead);

    let failure: GitError | undefined;
    try {
      await run(args);
    } catch (error) {
      if (!(error instanceof GitError)) throw error;
      failure = error;
    }
    // Git won't merge or rebase over a merge, rebase or conflicts that were already there, and
    // says so; those aren't the pull's to explain.
    if (!before.conflicts && !before.merging && !before.rebasing) {
      const after = await progress(run, repo.path);
      if (after.conflicts || after.merging || after.rebasing) {
        throw interruptedError(upstreamName(upstream), after, args, failure);
      }
    }
    if (failure) throw withoutHints(failure);
  });
}

interface Upstream {
  /** The remote's name (or URL); "." for a local branch. */
  remote: string;
  /** The upstream branch's full name on the remote, e.g. `refs/heads/main`. */
  merge: string;
  /** The rest of the settings a pull depends on, by key; see `readConfig`. */
  config: Map<string, string>;
}

/** What to merge or rebase onto: a ref, or a commit the fetch brought in. */
interface Target {
  rev: string;
  /** Whether `rev` is the upstream's remote-tracking branch, whose reflog finds a fork point. */
  tracking: boolean;
}

/** The checked-out branch, and the upstream it pulls from; rejects if there isn't one. */
async function readBranch(run: GitCommand): Promise<{ branch: string; upstream: Upstream }> {
  const [branch, config] = await Promise.all([currentBranch(run), readConfig(run)]);
  // From the config rather than `@{upstream}`, which also needs the remote-tracking branch:
  // that's missing before the first fetch, and the pull fetches it.
  const remote = branch === null ? undefined : config.get(`branch.${branch}.remote`);
  const merge = branch === null ? undefined : config.get(`branch.${branch}.merge`);
  const blocker = pullBlocker(branch, remote && merge ? merge : null);
  // Never without a blocker; the rest only tells TypeScript what that means.
  if (blocker || branch === null || !remote || !merge) throw new NoUpstreamError(blocker!);
  return { branch, upstream: { remote, merge, config } };
}

/**
 * Fetches `branch`'s upstream as `git pull` does: just that branch, which also updates its
 * remote-tracking branch. Most remotes map it to one (e.g. `origin/main`), which is then what's
 * merged; otherwise (a remote given as a URL, a narrower refspec) it's the commit fetched. A local
 * upstream (remote ".") has nothing to fetch.
 */
async function fetchUpstream(repo: Repo, branch: string, upstream: Upstream): Promise<Target> {
  const tracking = (
    await repo.read(["for-each-ref", "--format=%(upstream)", `refs/heads/${branch}`])
  ).trim();
  if (upstream.remote === "." && tracking) return { rev: tracking, tracking: true };

  // Without the upkeep a fetch may start (gc, maintenance, the commit-graph): it runs alongside
  // the queued writes, which that could get in the way of. And FETCH_HEAD only when it's needed.
  const config = ["gc.auto=0", "maintenance.auto=false", "fetch.writeCommitGraph=false"];
  if (tracking) config.push("fetch.writeFetchHead=false");
  try {
    await repo.remote(
      [
        ...config.flatMap((setting) => ["-c", setting]),
        "fetch",
        "--quiet",
        upstream.remote,
        upstream.merge,
      ],
      { env: remoteEnv(upstream.config) },
    );
  } catch (error) {
    throw error instanceof GitError ? withoutHints(error) : error;
  }
  if (tracking) return { rev: tracking, tracking: true };
  // Read straight away, before another fetch replaces it.
  const fetched = await repo.read(["rev-parse", "--verify", "--quiet", "FETCH_HEAD^{commit}"]);
  return { rev: fetched.trim(), tracking: false };
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
  const pattern = String.raw`^(pull\.(rebase|ff)|core\.sshcommand|http\.(.+\.)?lowspeed(limit|time)|branch\..+\.(remote|merge|rebase))$`;
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

/** How `upstream` is called in messages, e.g. `origin/main`. */
function upstreamName({ remote, merge }: Upstream): string {
  return remote === "." ? branchName(merge) : `${remote}/${branchName(merge)}`;
}

/** `ref`'s name as a branch, e.g. `main` for `refs/heads/main`. */
function branchName(ref: string): string {
  return ref.replace(/^refs\/heads\//, "");
}

const FALSE = new Set(["false", "no", "off", "0", ""]);

/**
 * The merge or rebase `git pull` would do after fetching, going by the same settings. Unlike
 * `git pull`, which refuses to pull a branch that diverged from its upstream until told whether
 * to merge or rebase, this merges then, like git did by default before 2.27. And it never rebases
 * interactively, which would open an editor with no one to use it.
 */
function integrateArgs(
  { remote, merge, config }: Upstream,
  branch: string,
  target: Target,
  hasHead: boolean,
): string[] {
  const rebase = (
    config.get(`branch.${branch}.rebase`) ?? config.get("pull.rebase")
  )?.toLowerCase();
  // A branch without commits yet has nothing to rebase: it's merged, which just moves it there.
  if (hasHead && rebase !== undefined && !FALSE.has(rebase)) {
    return [
      "rebase",
      "--quiet",
      ...(rebase === "merges" || rebase === "m" ? ["--rebase-merges"] : []),
      // From where the branch forked from its upstream, like `git pull --rebase`.
      ...(target.tracking ? ["--fork-point"] : []),
      target.rev,
    ];
  }
  const ff = config.get("pull.ff")?.toLowerCase();
  const ffArgs =
    !hasHead || ff === undefined
      ? []
      : ff === "only"
        ? ["--ff-only"]
        : FALSE.has(ff)
          ? ["--no-ff"]
          : ["--ff"];
  // A commit is named in the message as `git pull` names it; a ref names itself.
  const message = target.tracking ? [] : ["-m", `Merge branch '${branchName(merge)}' of ${remote}`];
  return ["merge", "--quiet", "--no-edit", ...ffArgs, ...message, target.rev];
}

/**
 * Keeps ssh from asking for a passphrase or to trust a host on the terminal Gitto may have been
 * started from (GIT_TERMINAL_PROMPT, see the runner, only covers HTTPS), unless there's an askpass
 * program to ask with instead, and gives up on an HTTP transfer that stalls. Settings of the
 * user's own win: their ssh command, and any HTTP low-speed setting. Exported for tests.
 */
export function remoteEnv(
  config: Map<string, string>,
  env: NodeJS.ProcessEnv = process.env,
): Record<string, string> {
  const result: Record<string, string> = {};
  if (env.SSH_ASKPASS) {
    // A program to ask with, e.g. the desktop's: always ask with it, never on the terminal.
    if (!env.SSH_ASKPASS_REQUIRE) result.SSH_ASKPASS_REQUIRE = "force";
  } else if (!env.GIT_SSH_COMMAND && !env.GIT_SSH && !config.has("core.sshcommand")) {
    // Only BatchMode: other options on the command line would override the user's ssh config.
    result.GIT_SSH_COMMAND = "ssh -o BatchMode=yes";
  }
  const hasLowSpeed =
    env.GIT_HTTP_LOW_SPEED_LIMIT ||
    env.GIT_HTTP_LOW_SPEED_TIME ||
    [...config.keys()].some((key) => /^http\..*lowspeed(limit|time)$/.test(key));
  if (!hasLowSpeed) {
    // Slower than 1KB/s for a minute: stalled.
    result.GIT_HTTP_LOW_SPEED_LIMIT = "1000";
    result.GIT_HTTP_LOW_SPEED_TIME = "60";
  }
  return result;
}

/** What a merge or rebase left unfinished in the repository at `path`. */
interface Progress {
  conflicts: boolean;
  merging: boolean;
  rebasing: boolean;
}

async function progress(run: GitCommand, path: string): Promise<Progress> {
  const [unmerged, merging, rebaseDirs] = await Promise.all([
    run(["ls-files", "--unmerged"]),
    refExists(run, "MERGE_HEAD"),
    run(["rev-parse", "--git-path", "rebase-merge", "--git-path", "rebase-apply"]),
  ]);
  // A rebase keeps its state in one of these while it's stopped, with or without conflicts.
  const rebasing = await Promise.all(
    rebaseDirs
      .split("\n")
      .filter(Boolean)
      .map((dir) =>
        // Relative to the repository, or absolute, e.g. in a linked worktree.
        stat(resolve(path, dir)).then(
          () => true,
          () => false,
        ),
      ),
  );
  return { conflicts: unmerged !== "", merging, rebasing: rebasing.includes(true) };
}

/** Says where the pull stopped, and what to do to finish it. */
function interruptedError(
  upstream: string,
  { conflicts, merging, rebasing }: Progress,
  args: string[],
  failure: GitError | undefined,
): PullInterruptedError {
  let message: string;
  if (rebasing && !conflicts) {
    // E.g. an untracked file in the way of a commit it replays.
    const reason = failure ? withoutHints(failure).message : "";
    message = `Rebasing onto ${upstream} stopped partway:\n${reason}\nFix that, then continue the rebase, or abort it.`;
  } else if (rebasing) {
    message = `Pulling ${upstream} caused conflicts. Resolve them, then continue the rebase.`;
  } else if (merging) {
    message = `Pulling ${upstream} caused conflicts. Resolve them, then commit the merge.`;
  } else {
    // The pull stashed local changes (rebase.autoStash, merge.autoStash) and they conflict.
    message = `Pulled ${upstream}, but your local changes conflict with it. Resolve the conflicts; your changes are also kept in the stash.`;
  }
  return new PullInterruptedError(message, args, failure?.exitCode ?? 0, failure?.stderr ?? "");
}

/** `error` without git's hints, which suggest commands to type and so don't help in the app. */
function withoutHints(error: GitError): GitError {
  const message = error.message
    .split("\n")
    .filter((line) => !line.startsWith("hint:"))
    .join("\n")
    .trim();
  if (!message || message === error.message) return error;
  return new GitError(message, error.args, error.exitCode, error.stderr);
}
