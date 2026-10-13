import { GitError, PushBlockedError, PushRejectedError } from "../../core/errors";
import { gitDirs } from "../../core/git-dirs";
import { currentBranch, resolveRef, type GitCommand, type Repo } from "../../core/repo";
import { mapRef, pushRemote, readConfig, remoteNames } from "./config";
import { whyDetached } from "./progress";
import type { PushTarget } from "./schema";

/** Where the checked-out branch is pushed (see `planPush`). */
interface PushPlan {
  branch: string;
  /** A remote's name, or a URL. */
  remote: string;
  /** Whether `remote` is a remote's name, rather than a URL. */
  named: boolean;
  /** The ref it's pushed to on the remote, e.g. `refs/heads/feature`. */
  ref: string;
  /**
   * Its remote-tracking branch, e.g. `refs/remotes/origin/feature`, which may not exist yet;
   * `null` if the remote's fetch refspecs don't fetch it into one.
   */
  tracking: string | null;
  /** Whether `ref` on `remote` is also what the branch pulls from. */
  pullsFrom: boolean;
  /** Whether the branch tracks nothing yet, and so tracks what it's pushed to once it is. */
  setUpstream: boolean;
}

/**
 * Where the checked-out branch is pushed: to its remote (see `pushRemote`), where the remote's push
 * refspecs say if it has any, and otherwise under the same name, as `git push` does by default; or
 * under its upstream's name if `push.default` says to push to the upstream, and that's on the same
 * remote. Only ever the one branch, whatever `push.default` says: the Push button names one.
 * Rejects with why it can't be pushed. `gitDir` is only asked for with HEAD detached.
 */
async function planPush(run: GitCommand, gitDir: () => Promise<string>): Promise<PushPlan> {
  const [branch, config, head] = await Promise.all([
    currentBranch(run),
    readConfig(run),
    resolveRef(run, "HEAD"),
  ]);
  if (branch === null) throw new PushBlockedError(await whyDetached(await gitDir(), "push"));
  if (head === null) throw new PushBlockedError(`${branch} has no commits to push yet.`);
  const remote = pushRemote(config, branch);
  if (remote === undefined) throw new PushBlockedError("There's no remote to push to.");

  const local = `refs/heads/${branch}`;
  const upstreamRemote = config.get(`branch.${branch}.remote`);
  const merge = config.get(`branch.${branch}.merge`);
  const pushDefault = config.get("push.default")?.toLowerCase();
  const pushSpecs = config.all(`remote.${remote}.push`);
  let ref: string | undefined;
  if (pushSpecs.length > 0) {
    ref = mapRef(pushSpecs, local, true);
    if (ref === undefined) {
      throw new PushBlockedError(`${remote}'s push settings don't push ${branch} anywhere.`);
    }
  } else if (
    merge?.startsWith("refs/heads/") &&
    upstreamRemote === remote &&
    (pushDefault === "upstream" || pushDefault === "tracking")
  ) {
    ref = merge;
  } else {
    ref = local;
  }
  // A URL, rather than a remote's name, has no settings, so no remote-tracking branches.
  const named = remoteNames(config, ["url", "pushurl"]).includes(remote);
  return {
    branch,
    remote,
    named,
    ref,
    tracking: named ? (mapRef(config.all(`remote.${remote}.fetch`), ref) ?? null) : null,
    pullsFrom: upstreamRemote === remote && merge === ref,
    setUpstream: !upstreamRemote || !merge,
  };
}

/** What to call the ref `plan` pushes to, e.g. `origin/feature`. */
function targetName({ remote, ref }: PushPlan): string {
  return `${remote}/${ref.replace(/^refs\/heads\//, "")}`;
}

/** Where the Push button pushes the checked-out branch, and how many commits it sends, or why not. */
export async function getPushTarget(repo: Repo): Promise<PushTarget> {
  let plan: PushPlan;
  try {
    plan = await planPush(repo.read, () => gitDirOf(repo));
  } catch (error) {
    if (error instanceof PushBlockedError) return { kind: "blocked", reason: error.message };
    throw error;
  }
  const tracked = plan.tracking !== null && (await resolveRef(repo.read, plan.tracking)) !== null;
  // What the remote doesn't have: past its branch, or, without that, past all of its branches
  // (all remotes' for a URL, which has none of its own).
  const known = tracked ? plan.tracking! : plan.named ? `--remotes=${plan.remote}` : "--remotes";
  const ahead = await repo.read(["rev-list", "--count", "HEAD", "--not", known]);
  return {
    kind: "remote",
    name: targetName(plan),
    isNew: plan.tracking !== null && !tracked,
    ahead: Number(ahead.trim()),
    pullsFrom: plan.pullsFrom,
  };
}

/**
 * Pushes the checked-out branch where `planPush` says; a branch that doesn't track one yet then
 * tracks it.
 *
 * With `force`, replaces the remote branch even if it has commits the branch doesn't, as after a
 * rebase or an amend; but only if they're ones the branch had: not if someone pushed others since
 * (`--force-with-lease`), even ones fetched in the background meanwhile (`--force-if-includes`).
 * Rejects with a `PushRejectedError` if the remote branch has commits the branch doesn't, which
 * says whether forcing would get past that.
 */
export async function push(repo: Repo, { force }: { force: boolean }): Promise<void> {
  const plan = await planPush(repo.read, () => gitDirOf(repo));
  const { branch, remote, ref } = plan;
  const args = [
    "push",
    // What happened to the ref, on stdout, to tell a rejection from other failures.
    "--porcelain",
    ...(force ? ["--force-with-lease", "--force-if-includes"] : []),
    "--",
    remote,
    `refs/heads/${branch}:${ref}`,
  ];
  try {
    // Queued with fetches rather than writes: it waits on the network, and only updates the
    // remote-tracking branch, which writes don't touch. Stopped if Gitto exits, as a fetch is:
    // the remote takes a ref's update whole or not at all, and pushing again finishes it.
    await repo.fetching((run) => run(args));
  } catch (error) {
    if (!(error instanceof GitError)) throw error;
    const reason = rejection(error.stdout, `refs/heads/${branch}`);
    const rejected = reason && (await rejectedError(repo.read, plan, reason));
    throw rejected ?? error.withoutHints();
  }
  if (plan.setUpstream) await setUpstream(repo, plan);
}

/**
 * Has the branch track what `plan` pushed it to, as `--set-upstream` would, but with the other
 * writes to the config rather than alongside them. Both settings or neither: the remote is set
 * last, and the ref taken back if that fails, as without it git takes the branch to track nothing.
 */
async function setUpstream(repo: Repo, { branch, remote, ref }: PushPlan): Promise<void> {
  await repo.exclusive(async (run) => {
    await run(["config", `branch.${branch}.merge`, ref]);
    try {
      await run(["config", `branch.${branch}.remote`, remote]);
    } catch (error) {
      await run(["config", "--unset", `branch.${branch}.merge`]).catch(() => undefined);
      throw error;
    }
  });
}

/** The repository's git directory. */
async function gitDirOf(repo: Repo): Promise<string> {
  return (await gitDirs(repo)).gitDir;
}

/**
 * Why git wouldn't push `ref` for what's on the remote, as `git push --porcelain` says on `stdout`:
 * e.g. "non-fast-forward". `undefined` if it didn't say: the remote itself may have declined it
 * (`[remote rejected]`), as a hook or `receive.denyNonFastForwards` does, which force-pushing
 * doesn't get past.
 */
export function rejection(stdout: string, ref: string): string | undefined {
  // Each ref on a line of its own: `!`, then `<from>:<to>`, then `[rejected] (<reason>)`, by tabs.
  for (const line of stdout.split("\n")) {
    const [flag, refs = "", summary = ""] = line.split("\t");
    const reason = /^\[rejected\] \((.*)\)$/.exec(summary)?.[1];
    if (flag === "!" && refs.split(":")[0] === ref && reason !== undefined) return reason;
  }
  return undefined;
}

/**
 * Why pushing as `plan` says was turned down for `reason` (see `rejection`), if that's for the
 * remote branch having commits the branch doesn't; `undefined` otherwise, which git explains itself.
 */
async function rejectedError(
  run: GitCommand,
  plan: PushPlan,
  reason: string,
): Promise<PushRejectedError | undefined> {
  const { branch, tracking } = plan;
  const target = targetName(plan);
  const notPulled = `${target} has commits you haven't pulled yet.`;
  switch (reason) {
    // Commits the repository has fetched: ones the branch had, before a rebase or an amend, say,
    // or someone else's.
    case "non-fast-forward":
      return (await hadCommit(run, branch, tracking))
        ? new PushRejectedError(
            `${target} still has ${branch}'s commits from before a rebase or amend.`,
            true,
          )
        : new PushRejectedError(notPulled, false);
    // Commits it hasn't fetched: someone else pushed to it.
    case "fetch first":
      return new PushRejectedError(notPulled, false);
    // A force-push that found it changed since it was fetched (`--force-with-lease`), or with
    // commits that were fetched but never in the branch (`--force-if-includes`).
    case "stale info":
    case "remote ref updated since checkout":
      return new PushRejectedError(`${notPulled.slice(0, -1)}, so it wasn't force-pushed.`, false);
    default:
      return undefined;
  }
}

/**
 * Whether `branch` ever had the commit `tracking` points at, or one after it: the check
 * `--force-if-includes` makes, so the force-push it's offered for goes through. Not without a
 * remote-tracking branch, which `--force-with-lease` needs too.
 */
async function hadCommit(
  run: GitCommand,
  branch: string,
  tracking: string | null,
): Promise<boolean> {
  if (tracking === null || (await resolveRef(run, tracking)) === null) return false;
  // Every commit the branch has pointed at, by its reflog; none if it keeps none.
  const reflog = await run(["log", "--walk-reflogs", "--format=%H", `refs/heads/${branch}`]).catch(
    () => "",
  );
  const tips = [...new Set(reflog.split("\n").filter(Boolean))];
  if (tips.length === 0) return false;
  // The commits in the remote branch's history that none of those has.
  const missing = await run(["rev-list", "--count", "--stdin"], {
    stdin: [tracking, ...tips.map((tip) => `^${tip}`)].join("\n") + "\n",
  });
  return Number(missing.trim()) === 0;
}
