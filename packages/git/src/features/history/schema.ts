import { z } from "zod";

import { RefKindSchema } from "../refs/schema";

/** A ref pointing at a commit: `HEAD`, or a branch or tag, e.g. local `main` or tag `v1`. */
export const CommitRefSchema = z.object({
  kind: z.enum(["head", ...RefKindSchema.options]),
  /** Short name, e.g. `main`, `origin/main`, `v1`; `HEAD` for the head. */
  name: z.string(),
});

export type CommitRef = z.infer<typeof CommitRefSchema>;

export const CommitSchema = z.object({
  sha: z.string(),
  parents: z.array(z.string()),
  authorName: z.string(),
  authorEmail: z.string(),
  /** Author date in milliseconds since the epoch. */
  authoredAt: z.number(),
  /** Refs pointing at this commit. When a branch is checked out, `HEAD` comes right before it. */
  refs: z.array(CommitRefSchema),
  subject: z.string(),
  body: z.string(),
});

export type Commit = z.infer<typeof CommitSchema>;
