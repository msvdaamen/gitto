import type { CommitRef } from "@/types/git";

/** A branch in the history's branch / tag column: local, on remotes, or both. */
export interface BranchLabel {
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

/** A label in the history's branch / tag column. */
export type RefLabel =
  | BranchLabel
  | { kind: "tag"; name: string }
  /** A detached HEAD. */
  | { kind: "head"; name: "HEAD" };

/**
 * Merges a commit's refs into labels, like GitKraken: a local branch and its remote copies
 * (`main`, `origin/main`) share one label. A detached HEAD comes first, then the checked-out
 * branch, other local branches, remote-only branches and tags.
 */
export function toRefLabels(refs: CommitRef[]): RefLabel[] {
  const head: RefLabel[] = refs.some((ref) => ref.kind === "head")
    ? [{ kind: "head", name: "HEAD" }]
    : [];
  const locals = new Map<string, BranchLabel>(
    refs
      .filter((ref) => ref.kind === "local")
      .map((ref) => [
        ref.name,
        { kind: "branch", name: ref.name, local: true, remotes: [], current: !!ref.current },
      ]),
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
    else remoteOnly.push({ kind: "branch", name: ref.name, local: false, remotes, current: false });
  }

  const tags = refs
    .filter((ref) => ref.kind === "tag")
    .map((ref): RefLabel => ({ kind: "tag", name: ref.name }));
  // The checked-out branch first; otherwise in git's order.
  const branches = [...locals.values()].toSorted((a, b) => Number(b.current) - Number(a.current));
  return [...head, ...branches, ...remoteOnly, ...tags];
}

/**
 * Whether a search matches the label: its name, a remote copy's full name (`origin/main`), `HEAD`
 * for the checked-out branch or, for a tag, `tag: v1`. Case-insensitive; `needle` is lowercase.
 */
export function refLabelMatches(label: RefLabel, needle: string): boolean {
  return searchNames(label).some((name) => name.toLowerCase().includes(needle));
}

function searchNames(label: RefLabel): string[] {
  if (label.kind === "tag") return [`tag: ${label.name}`];
  if (label.kind === "head" || !label.local) return [label.name];
  const remotes = label.remotes.map((remote) => `${remote}/${label.name}`);
  return [label.name, ...remotes, ...(label.current ? ["HEAD"] : [])];
}

/**
 * The label in words, e.g. `main (checked out; local, origin, upstream)`, `v1 (tag)` or
 * `origin/feature (remote)`; for tooltips, where its icons aren't shown.
 */
export function describeRefLabel(label: RefLabel): string {
  if (label.kind === "tag") return `${label.name} (tag)`;
  if (label.kind === "head") return "HEAD (detached)";
  if (!label.local) return `${label.name} (remote)`;
  const where = ["local", ...label.remotes].join(", ");
  return `${label.name} (${label.current ? `checked out; ${where}` : where})`;
}
