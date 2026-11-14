import { z } from "zod";

/** How far a clone has got, as git reports it. */
export const CloneProgressSchema = z.object({
  /** What git is doing, in its words: `Receiving objects`, `Resolving deltas`, `Updating files`… */
  phase: z.string(),
  /** How much of it is done, from 0 to 100. */
  percent: z.number().min(0).max(100),
});

export type CloneProgress = z.infer<typeof CloneProgressSchema>;
