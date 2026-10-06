import { z } from "zod";

/** A commit, as it's named to the user: its abbreviated SHA and its subject. */
export const CommitNameSchema = z.object({ sha: z.string(), subject: z.string() });

/** How far an operation that goes commit by commit has got: at `step` of `total`. */
const StepsSchema = z.object({ step: z.number(), total: z.number() }).nullable();

/**
 * An operation git left under way in the repository, for the user to finish or abort: a merge, a
 * rebase, cherry-picks or reverts, or `git am`. Names are as short as they can be said: `feature`,
 * `origin/main`, or an abbreviated SHA when no branch or tag points at the commit.
 */
export const OperationSchema = z.discriminatedUnion("kind", [
  z.object({
    kind: z.literal("merge"),
    /** What's being merged. */
    merging: z.string(),
    /** The branch it's merged into; `null` with HEAD detached. */
    into: z.string().nullable(),
  }),
  z.object({
    kind: z.literal("rebase"),
    /** The branch being rebased; `null` if it's a detached HEAD. */
    branch: z.string().nullable(),
    onto: z.string(),
    /** The commit being replayed, of how many. */
    steps: StepsSchema,
  }),
  z.object({
    kind: z.enum(["cherry-pick", "revert"]),
    /**
     * The commit being cherry-picked or reverted; `null` between the commits of a series, once
     * the one it stopped at was committed by hand.
     */
    commit: CommitNameSchema.nullable(),
    /** How many more are to be after it, in a series of them. */
    remaining: z.number(),
  }),
  z.object({
    kind: z.literal("am"),
    /** The patch being applied, of how many. */
    steps: StepsSchema,
  }),
]);

export type Operation = z.infer<typeof OperationSchema>;

export type OperationKind = Operation["kind"];

export const OperationKindSchema = z.enum(["merge", "rebase", "cherry-pick", "revert", "am"]);
