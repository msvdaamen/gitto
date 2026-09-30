import { parseRefName } from "../refs/parse";
import type { Commit, CommitRef } from "./schema";

const FIELDS = ["%H", "%P", "%an", "%ae", "%at", "%D", "%s", "%b"];

// With -z, commits are NUL-separated as well, so the output is a flat list of fields where every
// FIELDS.length entries make up one commit. None of the fields can contain NUL.
export const LOG_FORMAT = `--format=${FIELDS.join("%x00")}`;

export function parseLog(output: string): Commit[] {
  const fields = output.split("\0");
  const commits: Commit[] = [];

  for (let i = 0; i + FIELDS.length <= fields.length; i += FIELDS.length) {
    const [sha, parents, authorName, authorEmail, authoredAt, refs, subject, body] = fields.slice(
      i,
      i + FIELDS.length,
    ) as [string, string, string, string, string, string, string, string];

    commits.push({
      sha: sha.trim(),
      parents: parents ? parents.split(" ") : [],
      authorName,
      authorEmail,
      authoredAt: Number(authoredAt) * 1000,
      refs: parseDecorations(refs),
      subject,
      body: body.trim(),
    });
  }

  return commits;
}

/**
 * `HEAD -> refs/heads/main, refs/remotes/origin/main, tag: refs/tags/v1` → HEAD, local `main`,
 * remote `origin/main` and tag `v1`. Needs `--decorate=full`: short names can't tell a remote
 * branch from a local one named like it (`origin/main`).
 */
function parseDecorations(decorations: string): CommitRef[] {
  if (!decorations) return [];
  return decorations
    .split(", ")
    .flatMap((decoration) => decoration.split(" -> "))
    .flatMap<CommitRef>((ref) => {
      if (ref === "HEAD") return [{ kind: "head", name: "HEAD" }];
      return parseRefName(ref.replace(/^tag: /, "")) ?? [];
    });
}
