import { z } from "zod";

import { RefKindSchema } from "../refs/schema";

/**
 * A ref pointing at a commit: a branch or tag, e.g. local `main` or tag `v1`, or `HEAD` when it's
 * detached. A checked-out branch is marked `current` instead of getting a `HEAD` of its own.
 */
export const CommitRefSchema = z.object({
  kind: z.enum(["head", ...RefKindSchema.options]),
  /** Short name, e.g. `main`, `origin/main`, `v1`; `HEAD` for a detached head. */
  name: z.string(),
  /** Full name, e.g. `refs/heads/main`, `refs/remotes/origin/main`; `HEAD` for a detached head. */
  fullName: z.string(),
  /** Whether it's the checked-out branch. */
  current: z.boolean().optional(),
});

export type CommitRef = z.infer<typeof CommitRefSchema>;

export const CommitSchema = z.object({
  sha: z.string(),
  parents: z.array(z.string()),
  authorName: z.string(),
  authorEmail: z.string(),
  /** Author date in milliseconds since the epoch: when the change was first written. */
  authoredAt: z.number(),
  /**
   * Commit date in milliseconds since the epoch: when the commit was made, which a rebase or amend
   * moves to that moment. The history is sorted by it.
   */
  committedAt: z.number(),
  /** Refs pointing at this commit; a checked-out branch is marked `current`. */
  refs: z.array(CommitRefSchema),
  subject: z.string(),
  body: z.string(),
});

export type Commit = z.infer<typeof CommitSchema>;
