import { parseRefName } from "../refs/parse";
import type { Commit, CommitRef } from "./schema";

const FIELDS = ["%H", "%P", "%an", "%ae", "%at", "%ct", "%D", "%s", "%b"];

// With -z, commits are NUL-separated as well, so the output is a flat list of fields where every
// FIELDS.length entries make up one commit. None of the fields can contain NUL.
export const LOG_FORMAT = `--format=${FIELDS.join("%x00")}`;

export function parseLog(output: string): Commit[] {
  const fields = output.split("\0");
  const commits: Commit[] = [];

  for (let i = 0; i + FIELDS.length <= fields.length; i += FIELDS.length) {
    const [sha, parents, authorName, authorEmail, authoredAt, committedAt, refs, subject, body] =
      fields.slice(i, i + FIELDS.length) as [
        string,
        string,
        string,
        string,
        string,
        string,
        string,
        string,
        string,
      ];

    commits.push({
      sha: sha.trim(),
      parents: parents ? parents.split(" ") : [],
      authorName,
      authorEmail,
      authoredAt: Number(authoredAt) * 1000,
      committedAt: Number(committedAt) * 1000,
      refs: parseDecorations(refs),
      subject,
      body: body.trim(),
    });
  }

  return commits;
}

/**
 * `HEAD -> refs/heads/main, refs/remotes/origin/main, tag: refs/tags/v1` → the checked-out local
 * `main`, remote `origin/main` and tag `v1`. A detached HEAD is a bare `HEAD`, even on a commit a
 * branch points at too (`HEAD, refs/heads/main`). Needs `--decorate=full`: short names can't tell
 * a remote branch from a local one named like it (`origin/main`).
 */
function parseDecorations(decorations: string): CommitRef[] {
  if (!decorations) return [];
  return decorations.split(", ").flatMap<CommitRef>((decoration) => {
    if (decoration === "HEAD") return [{ kind: "head", name: "HEAD", fullName: "HEAD" }];
    const checkedOut = decoration.startsWith("HEAD -> ");
    const fullName = decoration.replace(/^HEAD -> |^tag: /, "");
    const ref = parseRefName(fullName);
    if (!ref) return [];
    return [checkedOut ? { ...ref, fullName, current: true } : { ...ref, fullName }];
  });
}
