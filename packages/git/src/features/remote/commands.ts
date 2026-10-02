import { GitError, NoUpstreamError, PullConflictError } from "../../core/errors";
import type { GitCommand, Repo } from "../../core/repo";
import { pullBlocker } from "./pull-blocker";

/**
 * Fetches the current branch's upstream and merges it in, or rebases onto it if the user's config
 * says to. Conflicts are left in the working tree, to resolve and commit.
 */
export async function pull(repo: Repo): Promise<void> {
  const [branch, config] = await Promise.all([currentBranch(repo), readConfig(repo)]);
  // The upstream is read from the config rather than `@{upstream}`, which also needs the
  // remote-tracking branch: that's missing before the first fetch, and the pull fetches it.
  const blocker = pullBlocker(branch, branch && (config.get(`branch.${branch}.merge`) ?? null));
  if (blocker) throw new NoUpstreamError(blocker);
  // Not detached: `pullBlocker` would have objected.
  const onBranch = branch!;

  // Quiet, so the fetch's progress doesn't take the place of the reason in an error.
  const args = [
    "pull",
    "--quiet",
    ...rebaseArgs(
      config.get(`branch.${onBranch}.rebase`) ?? config.get("pull.rebase"),
      config.has("pull.ff"),
    ),
  ];

  await repo.exclusive(async (run) => {
    // Git won't pull over unresolved conflicts, and says so; those aren't the pull's.
    const hadConflicts = await hasConflicts(run);
    let failure: GitError | undefined;
    try {
      await run(args, { env: remoteEnv(config) });
    } catch (error) {
      if (!(error instanceof GitError)) throw error;
      failure = error;
    }
    // Also after a pull that went through: putting back local changes it stashed can conflict.
    if (!hadConflicts && (await hasConflicts(run))) {
      throw await conflictError(run, onBranch, args, failure);
    }
    if (failure) throw withoutHints(failure);
  });
}

/**
 * Git refuses to pull a branch that diverged from its upstream until told whether to merge or
 * rebase. Merge, like git did by default before 2.27, unless the user's config says how (a
 * `pull.ff` says too, and `--no-rebase` would override `pull.ff=only`); but never rebase
 * interactively, which would open an editor with no one to use it.
 */
function rebaseArgs(rebase: string | undefined, hasFastForward: boolean): string[] {
  if (rebase === undefined) return hasFastForward ? [] : ["--no-rebase"];
  if (rebase === "interactive" || rebase === "i") return ["--rebase"];
  return [];
}

/**
 * Keeps ssh from asking for a passphrase or to trust a host on the terminal Gitto may have been
 * started from (GIT_TERMINAL_PROMPT, see the runner, only covers HTTPS), and gives up on a
 * connection that stalls, rather than holding up every other write to the repository. Settings of
 * the user's own win.
 */
function remoteEnv(config: Map<string, string>): Record<string, string> {
  const env: Record<string, string> = {};
  if (!process.env.GIT_SSH_COMMAND && !process.env.GIT_SSH && !config.has("core.sshcommand")) {
    env.GIT_SSH_COMMAND =
      "ssh -o BatchMode=yes -o ConnectTimeout=30 -o ServerAliveInterval=15 -o ServerAliveCountMax=4";
  }
  // Slower than 1KB/s for a minute: stalled.
  if (!process.env.GIT_HTTP_LOW_SPEED_LIMIT && !config.has("http.lowspeedlimit")) {
    env.GIT_HTTP_LOW_SPEED_LIMIT = "1000";
    env.GIT_HTTP_LOW_SPEED_TIME = "60";
  }
  return env;
}

/** The checked-out branch's name; `null` when HEAD is detached. */
async function currentBranch(repo: Repo): Promise<string | null> {
  try {
    return (await repo.read(["symbolic-ref", "--quiet", "--short", "HEAD"])).trim();
  } catch (error) {
    if (error instanceof GitError && error.exitCode === 1) return null;
    throw error;
  }
}

/**
 * The settings a pull depends on, in one read, by key. Git lowercases the section and name, but
 * not a branch's name: e.g. `branch.Feature.merge`. The last value of a key wins, as in git.
 */
async function readConfig(repo: Repo): Promise<Map<string, string>> {
  const pattern = String.raw`^(pull\.(rebase|ff)|core\.sshcommand|http\.lowspeedlimit|branch\..+\.(merge|rebase))$`;
  let output = "";
  try {
    output = await repo.read(["config", "-z", "--get-regexp", pattern]);
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

async function hasConflicts(run: GitCommand): Promise<boolean> {
  return (await run(["ls-files", "--unmerged"])) !== "";
}

/** Says what the pull stopped at, and what to do once the conflicts are resolved. */
async function conflictError(
  run: GitCommand,
  branch: string,
  args: string[],
  failure: GitError | undefined,
): Promise<PullConflictError> {
  const exists = (ref: string) =>
    run(["rev-parse", "--quiet", "--verify", ref]).then(
      () => true,
      () => false,
    );
  const [upstream, merging, rebasing] = await Promise.all([
    // By the branch's name: HEAD is detached while a rebase stops.
    run(["for-each-ref", "--format=%(upstream:short)", `refs/heads/${branch}`]).then(
      (name) => name.trim() || "the upstream",
      () => "the upstream",
    ),
    exists("MERGE_HEAD"),
    exists("REBASE_HEAD"),
  ]);
  const message = merging
    ? `Pulling ${upstream} caused conflicts. Resolve them, then commit the merge.`
    : rebasing
      ? `Pulling ${upstream} caused conflicts. Resolve them, then continue the rebase.`
      : // The pull stashed local changes (rebase.autoStash, merge.autoStash) and they conflict.
        `Pulled ${upstream}, but your local changes conflict with it. Resolve the conflicts; your changes are also kept in the stash.`;
  return new PullConflictError(message, args, failure?.exitCode ?? 0, failure?.stderr ?? "");
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
