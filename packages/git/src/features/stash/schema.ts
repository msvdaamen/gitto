import { z } from "zod";

export const StashSchema = z.object({
  /** The stash commit's full SHA. */
  sha: z.string(),
  /**
   * The commit it was made on, which its changes are compared to; the first of the stash commit's
   * parents (the others hold what was staged, and the untracked files).
   */
  base: z.string(),
  /** What git named it, e.g. `WIP on main: 1a2b3c4 Fix the header`. */
  message: z.string(),
  /** When it was made, in milliseconds since the epoch. */
  createdAt: z.number(),
});

export type Stash = z.infer<typeof StashSchema>;
