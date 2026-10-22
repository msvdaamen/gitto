import { z } from "zod";

/**
 * What merging a branch did: nothing, as the branch had every commit already; moved the branch to
 * it; made a merge commit; or stopped at conflicts, left to resolve and commit.
 */
export const MergeOutcomeSchema = z.enum(["up-to-date", "fast-forward", "merged", "conflicts"]);

export type MergeOutcome = z.infer<typeof MergeOutcomeSchema>;

/**
 * What rebasing a branch did: nothing, as it was on top of the other already; moved it to the
 * other, having no commits of its own to replay (or none the other didn't have already, which git
 * leaves out); replayed its commits on top of it; or stopped at conflicts, left to resolve and
 * continue.
 */
export const RebaseOutcomeSchema = z.enum(["up-to-date", "fast-forward", "rebased", "conflicts"]);

export type RebaseOutcome = z.infer<typeof RebaseOutcomeSchema>;
