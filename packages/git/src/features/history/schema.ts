import { z } from "zod";

export const CommitSchema = z.object({
  sha: z.string(),
  parents: z.array(z.string()),
  authorName: z.string(),
  authorEmail: z.string(),
  /** Author date in milliseconds since the epoch. */
  authoredAt: z.number(),
  /** Short ref names pointing at this commit, e.g. `HEAD`, `main`, `origin/main`, `tag: v1`. */
  refs: z.array(z.string()),
  subject: z.string(),
  body: z.string(),
});

export type Commit = z.infer<typeof CommitSchema>;
