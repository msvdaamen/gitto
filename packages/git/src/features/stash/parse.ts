import type { Stash } from "./schema";

const FIELDS = ["%H", "%P", "%ct", "%gs"];

/** How `stash list -z` is read: every field and entry ends in a NUL, which none of them can contain. */
export const STASH_LIST_FORMAT = `--format=${FIELDS.join("%x00")}`;

/** The stashes in `stash list`'s output (see `STASH_LIST_FORMAT`), newest first. */
export function parseStashList(output: string): Stash[] {
  const fields = output.split("\0");
  const stashes: Stash[] = [];
  for (let i = 0; i + FIELDS.length <= fields.length; i += FIELDS.length) {
    const [sha = "", parents = "", createdAt = "", message = ""] = fields.slice(
      i,
      i + FIELDS.length,
    );
    const base = parents.split(" ")[0]!;
    stashes.push({ sha, base, message, createdAt: Number(createdAt) * 1000 });
  }
  return stashes;
}
