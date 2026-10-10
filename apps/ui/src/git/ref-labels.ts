import type { CommitRef, Worktree } from "@gitto/git/types";

/** A branch in the history's branch / tag column: local, on remotes, or both. */
export interface BranchLabel {
  kind: "branch";
  /** `main` for a local branch (and its remote copies), `origin/feature` for a remote one. */
  name: string;
  /**
   * The full ref name to switch to: the local branch's, e.g. `refs/heads/main`, or for a remote one
   * `refs/remotes/origin/feature` (switching to it switches to the local branch tracking it).
   */
  ref: string;
  /** Whether a local branch points here. */
  local: boolean;
  /** Remotes whose copy of the branch points here, e.g. `origin`. */
  remotes: string[];
  /** Whether it's the checked-out branch. */
  current: boolean;
  /**
   * The other worktree it's checked out in, if one is, named after its folder: a branch is checked
   * out in one worktree at a time.
   */
  worktree?: WorktreeAt;
}

/** A worktree, by its folder's name and path. */
export interface WorktreeAt {
  name: string;
  path: string;
}

/** A label in the history's branch / tag column. */
export type RefLabel =
  | BranchLabel
  | { kind: "tag"; name: string }
  /** A detached HEAD. */
  | { kind: "head"; name: "HEAD" }
  /** Another worktree, named after its folder, checked out at the commit with no branch. */
  | ({ kind: "worktree" } & WorktreeAt);

/** Where the other worktrees are checked out, for the labels to show. */
export interface Checkouts {
  /** Each worktree, by the full ref name of the branch checked out in it. */
  branches: ReadonlyMap<string, WorktreeAt>;
  /** The worktrees checked out at each commit with no branch, by its SHA. */
  detached: ReadonlyMap<string, WorktreeAt[]>;
}

export const NO_CHECKOUTS: Checkouts = { branches: new Map(), detached: new Map() };

/**
 * Where the worktrees other than the one on show are checked out (see `Checkouts`); a bare one
 * isn't checked out anywhere.
 */
export function checkouts(worktrees: readonly Worktree[]): Checkouts {
  const branches = new Map<string, WorktreeAt>();
  const detached = new Map<string, WorktreeAt[]>();
  for (const worktree of worktrees) {
    if (worktree.current || worktree.bare) continue;
    const at = { name: worktree.name, path: worktree.path };
    if (worktree.branch) branches.set(worktree.branch, at);
    else if (worktree.head) {
      const there = detached.get(worktree.head);
      if (there) there.push(at);
      else detached.set(worktree.head, [at]);
    }
  }
  return { branches, detached };
}

/**
 * Merges the refs of the commit `sha` into labels, like GitKraken: a local branch and its remote
 * copies (`main`, `origin/main`) share one label. A detached HEAD comes first, then the
 * checked-out branch, other local branches, the other worktrees checked out at the commit with no
 * branch (`others`), remote-only branches and tags. A branch checked out in another worktree
 * says which.
 */
export function toRefLabels(
  refs: CommitRef[],
  sha = "",
  others: Checkouts = NO_CHECKOUTS,
): RefLabel[] {
  const head: RefLabel[] = refs.some((ref) => ref.kind === "head")
    ? [{ kind: "head", name: "HEAD" }]
    : [];
  const worktrees = (others.detached.get(sha) ?? []).map((at): RefLabel => ({
    kind: "worktree",
    name: at.name,
    path: at.path,
  }));
  const locals = new Map<string, BranchLabel>(
    refs
      .filter((ref) => ref.kind === "local")
      .map((ref) => {
        const worktree = others.branches.get(ref.fullName);
        return [
          ref.name,
          {
            kind: "branch",
            name: ref.name,
            ref: ref.fullName,
            local: true,
            remotes: [],
            current: !!ref.current,
            ...(worktree === undefined ? {} : { worktree }),
          },
        ];
      }),
  );
  // Remote branches without a local copy here; kept apart from the local ones, as a local branch
  // can be named like a remote one (`origin/main`).
  const remoteOnly: BranchLabel[] = [];

  for (const ref of refs) {
    if (ref.kind !== "remote") continue;
    // `origin/feature/login` is branch `feature/login` on remote `origin`.
    const slash = ref.name.indexOf("/");
    const remotes = slash === -1 ? [] : [ref.name.slice(0, slash)];
    const local = slash === -1 ? undefined : locals.get(ref.name.slice(slash + 1));
    if (local) local.remotes.push(...remotes);
    else {
      remoteOnly.push({
        kind: "branch",
        name: ref.name,
        ref: ref.fullName,
        local: false,
        remotes,
        current: false,
      });
    }
  }

  const tags = refs
    .filter((ref) => ref.kind === "tag")
    .map((ref): RefLabel => ({ kind: "tag", name: ref.name }));
  // The checked-out branch first; otherwise in git's order.
  const branches = [...locals.values()].toSorted((a, b) => Number(b.current) - Number(a.current));
  return [...head, ...branches, ...worktrees, ...remoteOnly, ...tags];
}

/**
 * Whether a search matches the label: its name, a remote copy's full name (`origin/main`), `HEAD`
 * for the checked-out branch, the worktree a branch is checked out in or, for a tag, `tag: v1`.
 * Case-insensitive; `needle` is lowercase.
 */
export function refLabelMatches(label: RefLabel, needle: string): boolean {
  return searchNames(label).some((name) => name.toLowerCase().includes(needle));
}

function searchNames(label: RefLabel): string[] {
  if (label.kind === "tag") return [`tag: ${label.name}`];
  if (label.kind === "head" || label.kind === "worktree" || !label.local) return [label.name];
  const remotes = label.remotes.map((remote) => `${remote}/${label.name}`);
  const worktree = label.worktree === undefined ? [] : [label.worktree.name];
  return [label.name, ...remotes, ...(label.current ? ["HEAD"] : []), ...worktree];
}

/**
 * The label in words, e.g. `main (checked out; local, origin, upstream)`, `v1 (tag)`,
 * `origin/feature (remote)`, `feature (local; checked out in gitto-feature)` or
 * `gitto-check (worktree, detached)`; for tooltips, where its icons aren't shown.
 */
export function describeRefLabel(label: RefLabel): string {
  if (label.kind === "tag") return `${label.name} (tag)`;
  if (label.kind === "head") return "HEAD (detached)";
  if (label.kind === "worktree") return `${label.name} (worktree, detached)`;
  if (!label.local) return `${label.name} (remote)`;
  const where = ["local", ...label.remotes].join(", ");
  if (label.worktree !== undefined) {
    return `${label.name} (${where}; checked out in ${label.worktree.name})`;
  }
  return `${label.name} (${label.current ? `checked out; ${where}` : where})`;
}
