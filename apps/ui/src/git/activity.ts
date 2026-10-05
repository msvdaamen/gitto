import type { Activity } from "@gitto/git/types";

/** A full commit SHA, as git names one when there's no branch to name. */
const FULL_SHA = /^[0-9a-f]{40}([0-9a-f]{24})?$/;

/** A branch or commit as git named it, with a commit's SHA shortened. */
function shortName(target: string): string {
  return FULL_SHA.test(target) ? target.slice(0, 7) : target;
}

/**
 * How to tell something done in a repository: what to show it as (the commit's subject, for one
 * that made a commit) and, for those, what was done to make it.
 */
export function describeActivity(activity: Activity): { title: string; action?: string } {
  const target = activity.target && shortName(activity.target);
  const named = (verb: string) => (target ? `${verb} ${target}` : verb);
  switch (activity.kind) {
    case "commit":
      return { title: activity.subject, action: "Committed" };
    case "amend":
      return { title: activity.subject, action: "Amended" };
    case "cherry-pick":
      return { title: activity.subject, action: "Cherry-picked" };
    case "revert":
      return { title: activity.subject, action: "Reverted" };
    case "merge":
      return { title: named("Merged") };
    case "pull":
      return { title: named("Pulled") };
    case "rebase":
      return { title: named("Rebased") };
    case "checkout":
      return { title: named("Switched to") };
    case "reset":
      return { title: named("Reset to") };
    case "clone":
      return { title: "Cloned" };
    case "other":
      // Git's words, with branches by their names, e.g. "Branch: renamed main to trunk".
      return { title: activity.message.replaceAll("refs/heads/", "") };
  }
}
