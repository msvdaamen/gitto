import { z } from "zod";

/** Where the Push button pushes the checked-out branch, or why it can't. */
export const PushTargetSchema = z.discriminatedUnion("kind", [
  /** Not on a branch, a branch without commits, or no remote to push to. */
  z.object({ kind: z.literal("blocked"), reason: z.string() }),
  z.object({
    kind: z.literal("remote"),
    /** The remote branch, by its remote's name and its own, e.g. `origin/feature`. */
    name: z.string(),
    /**
     * Whether it's not there yet, as of the last fetch or push, so pushing makes it. Never for
     * one that isn't fetched into a remote-tracking branch, which can't tell.
     */
    isNew: z.boolean(),
    /** How many commits pushing would send, as of the last fetch or push. */
    ahead: z.number(),
    /** Whether it's also the branch pulled from, so pulling brings in its commits. */
    pullsFrom: z.boolean(),
  }),
]);

export type PushTarget = z.infer<typeof PushTargetSchema>;
