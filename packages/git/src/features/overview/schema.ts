import { z } from "zod";

/** What moved HEAD, going by its reflog. */
export const ActivityKindSchema = z.enum([
  "commit",
  "amend",
  "merge",
  "pull",
  "rebase",
  "checkout",
  "reset",
  "cherry-pick",
  "revert",
  "clone",
  "other",
]);

export type ActivityKind = z.infer<typeof ActivityKindSchema>;

/** Something done in the repository that moved HEAD: an entry in its reflog. */
export const ActivitySchema = z.object({
  /** When, in milliseconds since the epoch. */
  at: z.number(),
  kind: ActivityKindSchema,
  /**
   * What it was done with, as git names it: the branch or commit switched or reset to, what was
   * merged or pulled, the branch rebased. `null` if git doesn't name one.
   */
  target: z.string().nullable(),
  /** The subject of the commit HEAD was at after it, e.g. the one just made. */
  subject: z.string(),
  /** Git's own words for it, e.g. `checkout: moving from main to feature`. */
  message: z.string(),
});

export type Activity = z.infer<typeof ActivitySchema>;

/** What the home page shows of a repository, beyond its status. */
export const OverviewSchema = z.object({
  /** The remote it's shown as hosted on: `origin`, or else the first one; `null` without any. */
  remote: z.object({ name: z.string(), url: z.string() }).nullable(),
  /** When a remote was last fetched from, in milliseconds since the epoch; `null` if never. */
  fetchedAt: z.number().nullable(),
  /** When HEAD's commit was made, in milliseconds since the epoch; `null` before the first one. */
  committedAt: z.number().nullable(),
  /** What was last done in the repository, most recent first. */
  activity: z.array(ActivitySchema),
});

export type Overview = z.infer<typeof OverviewSchema>;
