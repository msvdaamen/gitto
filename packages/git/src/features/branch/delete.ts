import { BranchDeletionBlockedError, GitError } from "../../core/errors";
import type { GitCommand, Repo } from "../../core/repo";
import { parseRefName } from "../refs/parse";
import { listRemotes } from "../remote/commands";
import { remoteOf } from "../remote/remote-name";
import { trackingLocals } from "./commands";
import { branchesInUse } from "./in-use";

/**
 * Deletes the branch `ref` (a full ref name), with the commits only it has. A local branch is
 * deleted here only; git refuses one that's checked out, or being rebased or bisected, in any
 * worktree, saying so.
 *
 * A remote branch is deleted on its remote, the branch there found as the remote's fetch settings
 * map it to `ref`, then the local branches that track it are; or, if the remote's gone, only what's
 * left of it here. Nothing is deleted if git wouldn't delete one of those local branches, nor if
 * the remote's settings don't fetch `ref` from a branch there. Should a local branch still fail to
 * be deleted after the remote one was, checked out in a terminal meanwhile say, that's thrown,
 * saying the remote one is gone.
 */
export async function deleteBranch(repo: Repo, ref: string): Promise<void> {
  const target = parseRefName(ref);
  if (!target || target.kind === "tag") throw new Error(`${ref} isn't a branch.`);
  if (target.kind === "local") {
    await repo.write(["branch", "--delete", "--force", "--", target.name]);
    return;
  }

  // One write, from finding the local branches tracking it to deleting them, so no other write
  // (a switch, say) can check one out in between, once the remote one is gone. That holds up the
  // other writes while the remote answers, which a deletion, asked for and waited on, can afford.
  await repo.exclusive(async (run) => {
    const [tracking, inUse, fetched, remotes] = await Promise.all([
      trackingLocals(run, ref),
      branchesInUse(run),
      fetchedFrom(run, ref),
      listRemotes(run),
    ]);
    for (const { name } of tracking) {
      const use = inUse.get(`refs/heads/${name}`);
      if (use) {
        throw new BranchDeletionBlockedError(
          `${name} tracks ${target.name} and ${use.state}, so neither was deleted. ${use.fix}`,
        );
      }
    }

    if (fetched) {
      // As a full ref name, which the remote deletes even if it's gone there already: a short one
      // that's no longer there fails. Pushing also drops the remote-tracking branch here. In turn
      // with the fetches too, which change the remote-tracking branches as well, and stopped if
      // Gitto exits, as they are. The pre-push hook runs, as for any push: it's the user's say
      // what may be pushed.
      try {
        await repo.fetch(["push", "--quiet", "--delete", "--", fetched.remote, fetched.branch]);
      } catch (error) {
        throw error instanceof GitError ? error.withoutHints() : error;
      }
    } else {
      const remote = remoteOf(remotes, target.name);
      if (remote) {
        throw new BranchDeletionBlockedError(
          `${remote}'s fetch settings don't fetch ${target.name} from a branch there, so nothing was deleted.`,
        );
      }
      await run(["branch", "--delete", "--remotes", "--force", "--", target.name]);
    }
    if (tracking.length === 0) return;
    try {
      await run(["branch", "--delete", "--force", "--", ...tracking.map(({ name }) => name)]);
    } catch (error) {
      if (!fetched || !(error instanceof GitError)) throw error;
      throw error.withMessage(
        `${target.name} was deleted on ${fetched.remote}, but not every local branch tracking it was. ${error.message}`,
      );
    }
  });
}

/**
 * The remote, and the branch on it, that fetching updates the remote-tracking branch `ref` (a full
 * ref name) from, as the remotes' fetch refspecs say; `undefined` if none of them does.
 */
async function fetchedFrom(
  run: GitCommand,
  ref: string,
): Promise<{ remote: string; branch: string } | undefined> {
  // Each setting as its key, a newline, and its value, NUL-terminated; exits with 1 if there are
  // none.
  const settings = await run([
    "config",
    "-z",
    "--get-regexp",
    String.raw`^remote\..*\.fetch$`,
  ]).catch((error: unknown) => {
    if (error instanceof GitError && error.exitCode === 1) return "";
    throw error;
  });
  for (const setting of settings.split("\0")) {
    const newline = setting.indexOf("\n");
    if (newline === -1) continue;
    const branch = fetchedInto(setting.slice(newline + 1), ref);
    if (branch)
      return { remote: setting.slice("remote.".length, newline - ".fetch".length), branch };
  }
  return undefined;
}

/**
 * The ref the fetch refspec `spec` fetches into `ref`, e.g. `refs/heads/main` for
 * `refs/remotes/origin/main` with `+refs/heads/*:refs/remotes/origin/*`; `undefined` if it
 * doesn't fetch into `ref`, or only says what not to fetch (`^refs/heads/x`).
 */
export function fetchedInto(spec: string, ref: string): string | undefined {
  if (spec.startsWith("^")) return undefined;
  const [source, destination] = spec.replace(/^\+/, "").split(":");
  if (!source || !destination) return undefined;
  const star = destination.indexOf("*");
  if (star === -1) return destination === ref ? source : undefined;
  const before = destination.slice(0, star);
  const after = destination.slice(star + 1);
  if (
    ref.length <= before.length + after.length ||
    !ref.startsWith(before) ||
    !ref.endsWith(after)
  ) {
    return undefined;
  }
  return source.replace("*", ref.slice(before.length, ref.length - after.length));
}
