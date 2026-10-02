import { stat } from "node:fs/promises";
import { resolve } from "node:path";

import {
  GitError,
  NoUpstreamError,
  PullInterruptedError,
  RepositoryChangedError,
} from "../../core/errors";
import { refExists, type GitCommand, type Repo } from "../../core/repo";
import { NO_BRANCH, noUpstream } from "./pull-blocker";

/**
 * Fetches every remote, and drops remote-tracking branches whose branch was deleted on the remote,
 * so the sidebar and history show the remotes as they are.
 */
export async function fetchAll(repo: Repo): Promise<void> {
  await fetch(repo, ["--all", "--prune", "--no-progress"]);
}

/**
 * Runs `git fetch` with `args`, without the upkeep it may start (gc, maintenance, the
 * commit-graph): fetches run alongside the queued writes, which that could get in the way of.
 * `settings` adds to those; `config` is the repository's, if it's been read already.
 */
async function fetch(
  repo: Repo,
  args: string[],
  { settings = [], config }: { settings?: string[]; config?: Map<string, string> } = {},
): Promise<void> {
  const all = ["gc.auto=0", "maintenance.auto=false", "fetch.writeCommitGraph=false", ...settings];
  try {
    await repo.fetch([...all.flatMap((setting) => ["-c", setting]), "fetch", ...args], {
      env: remoteEnv(config ?? (await readConfig(repo.read))),
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
  // Just the upstream branch, as `git pull` fetches it, which updates its remote-tracking branch.
  // A local upstream (remote ".") has nothing to fetch.
  if (upstream.remote !== ".") {
    await fetch(repo, ["--quiet", upstream.remote, upstream.merge], {
      settings: ["fetch.writeFetchHead=false"],
      config: upstream.config,
    });
  }

  await repo.exclusive(async (run) => {
    // Read again, now that no other write can change them: one may have while fetching.
    const [current, tracking, before, hasHead] = await Promise.all([
      readBranch(run, repo.path),
      trackingBranch(run, branch),
      progress(run, repo.path),
      refExists(run, "HEAD"),
    ]);
    if (current.branch !== branch) {
      throw new RepositoryChangedError(`Switched from ${branch} while pulling it. Pull again.`);
    }
    if (current.upstream.remote !== upstream.remote || current.upstream.merge !== upstream.merge) {
      throw new RepositoryChangedError(
        `${branch}'s upstream changed while pulling it. Pull again.`,
      );
    }
    if (!tracking) throw new NoUpstreamError(noUpstream(branch));
    const args = integrateArgs(current.upstream.config, branch, tracking, hasHead);

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
  /** The rest of the settings a pull depends on, by key; see `readConfig`. */
  config: Map<string, string>;
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
    throw new NoUpstreamError(
      rebasing ? "A rebase is under way. Continue or abort it, then pull." : NO_BRANCH,
    );
  }
  // From the config: a branch without commits yet has no ref to ask for its upstream.
  const remote = config.get(`branch.${branch}.remote`);
  const merge = config.get(`branch.${branch}.merge`);
  if (!remote || !merge) throw new NoUpstreamError(noUpstream(branch));
  return { branch, upstream: { remote, merge, config } };
}

/**
 * The remote-tracking branch `branch`'s upstream was fetched into, e.g. `refs/remotes/origin/main`
 * (or the local branch it tracks). `null` without one, as for an upstream given as a URL: then
 * `git status` reports no upstream either, so the UI doesn't offer to pull.
 */
function trackingBranch(run: GitCommand, branch: string): Promise<string | null> {
  return run(["rev-parse", "--symbolic-full-name", `${branch}@{upstream}`]).then(
    (ref) => ref.trim() || null,
    () => null,
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
      "--fork-point",
      tracking,
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
  return ["merge", "--quiet", "--no-edit", ...ffArgs, tracking];
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
    const reason = failure && withoutHints(failure).message;
    message = reason
      ? `Rebasing onto ${upstream} stopped partway:\n${reason}\nFix that, then continue the rebase, or abort it.`
      : `Rebasing onto ${upstream} stopped partway. Continue the rebase, or abort it.`;
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
