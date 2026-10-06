import { z } from "zod";

/**
 * What merging a branch did: nothing, as the branch had every commit already; moved the branch to
 * it; made a merge commit; or stopped at conflicts, left to resolve and commit.
 */
export const MergeOutcomeSchema = z.enum(["up-to-date", "fast-forward", "merged", "conflicts"]);

export type MergeOutcome = z.infer<typeof MergeOutcomeSchema>;
