import type { CommitRef } from "@/types/git";

/** A label in the history's branch / tag column. */
export type RefLabel =
  | {
      kind: "branch";
      /** `main` for a local branch (and its remote copies), `origin/feature` for a remote one. */
      name: string;
      /** Whether a local branch points here. */
      local: boolean;
      /** Remotes whose copy of the branch points here, e.g. `origin`. */
      remotes: string[];
      /** Whether it's the checked-out branch. */
      current: boolean;
    }
  | { kind: "tag"; name: string }
  /** A detached HEAD. */
  | { kind: "head"; name: "HEAD" };

/**
 * Merges a commit's refs into labels, like GitKraken: a local branch and its remote copies
 * (`main`, `origin/main`) share one label. The checked-out branch comes first, then other local
 * branches, remote-only branches and tags.
 */
export function toRefLabels(refs: CommitRef[]): RefLabel[] {
  const branches = new Map<string, Extract<RefLabel, { kind: "branch" }>>();
  const tags: RefLabel[] = [];
  let head: RefLabel | undefined;

  refs.forEach((ref, index) => {
    if (ref.kind === "head") {
      // `HEAD` comes right before the branch it's on; without one it's detached.
      if (refs[index + 1]?.kind !== "local") head = { kind: "head", name: "HEAD" };
    } else if (ref.kind === "tag") {
      tags.push({ kind: "tag", name: ref.name });
    } else if (ref.kind === "local") {
      const current = refs[index - 1]?.kind === "head";
      branches.set(ref.name, { kind: "branch", name: ref.name, local: true, remotes: [], current });
    }
  });

  // Remotes after locals, so a remote copy finds its local branch whichever comes first.
  for (const ref of refs) {
    if (ref.kind !== "remote") continue;
    const slash = ref.name.indexOf("/");
    const local = branches.get(ref.name.slice(slash + 1));
    if (slash !== -1 && local?.local) {
      local.remotes.push(ref.name.slice(0, slash));
    } else {
      branches.set(ref.name, {
        kind: "branch",
        name: ref.name,
        local: false,
        remotes: [ref.name.slice(0, slash)],
        current: false,
      });
    }
  }

  const rank = (label: Extract<RefLabel, { kind: "branch" }>) =>
    label.current ? 0 : label.local ? 1 : 2;
  const sorted = [...branches.values()].toSorted((a, b) => rank(a) - rank(b));
  return [...(head ? [head] : []), ...sorted, ...tags];
}
