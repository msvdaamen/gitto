import type { Ref } from "./schema";

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

export function parseRefs(output: string): Ref[] {
  const refs: Ref[] = [];

  for (const line of output.split("\n")) {
    if (!line) continue;
    const [fullName = "", sha = "", upstream = "", track = "", head = ""] = line.split("\0");

    const prefix = PREFIXES.find(([p]) => fullName.startsWith(p));
    if (!prefix) continue;
    // `refs/remotes/origin/HEAD` is a symbolic ref to the remote's default branch, not a branch.
    if (prefix[1] === "remote" && fullName.endsWith("/HEAD")) continue;

    refs.push({
      name: fullName.slice(prefix[0].length),
      fullName,
      kind: prefix[1],
      sha,
      current: head === "*",
      upstream: upstream || null,
      ahead: Number(/ahead (\d+)/.exec(track)?.[1] ?? 0),
      behind: Number(/behind (\d+)/.exec(track)?.[1] ?? 0),
    });
  }

  return refs;
}
