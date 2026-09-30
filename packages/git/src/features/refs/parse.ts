import type { Ref, RefKind } from "./schema";

// Ref names can't contain newlines or NUL, so one line per ref with NUL-separated fields is safe.
export const REFS_ARGS = [
  "for-each-ref",
  "--format=%(refname)%00%(objectname)%00%(upstream:short)%00%(upstream:track,nobracket)%00%(HEAD)",
  "refs/heads",
  "refs/remotes",
  "refs/tags",
];

const PREFIXES = [
  ["refs/heads/", "local"],
  ["refs/remotes/", "remote"],
  ["refs/tags/", "tag"],
] as const;

/**
 * The kind and short name of a full ref name, e.g. `refs/remotes/origin/main` → remote
 * `origin/main`. `null` for refs that aren't branches or tags, like `refs/stash`, and for
 * `refs/remotes/<remote>/HEAD`: a symbolic ref to the remote's default branch, not a branch.
 */
export function parseRefName(fullName: string): { kind: RefKind; name: string } | null {
  const prefix = PREFIXES.find(([p]) => fullName.startsWith(p));
  if (!prefix) return null;
  const [start, kind] = prefix;
  if (kind === "remote" && fullName.endsWith("/HEAD")) return null;
  return { kind, name: fullName.slice(start.length) };
}

export function parseRefs(output: string): Ref[] {
  const refs: Ref[] = [];

  for (const line of output.split("\n")) {
    if (!line) continue;
    const [fullName = "", sha = "", upstream = "", track = "", head = ""] = line.split("\0");

    const ref = parseRefName(fullName);
    if (!ref) continue;

    refs.push({
      ...ref,
      fullName,
      sha,
      current: head === "*",
      upstream: upstream || null,
      ahead: Number(/ahead (\d+)/.exec(track)?.[1] ?? 0),
      behind: Number(/behind (\d+)/.exec(track)?.[1] ?? 0),
    });
  }

  return refs;
}
