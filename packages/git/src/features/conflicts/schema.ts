import { z } from "zod";

/**
 * One side of a conflicted file, as the index has it while it's unmerged: its mode (`100644`, say,
 * or `120000` for a symbolic link and `160000` for a submodule) and its object.
 */
export const ConflictSideSchema = z.object({ mode: z.string(), oid: z.string() });

export type ConflictSide = z.infer<typeof ConflictSideSchema>;

/**
 * The sides of a conflicted file: `null` where the file isn't there, e.g. deleted on one side, or
 * added on both, without a common ancestor's. Ours is HEAD's: the branch merged into, or, in a
 * rebase, the one rebased onto; theirs is what's merged, cherry-picked, or replayed.
 */
export const ConflictSidesSchema = z.object({
  base: ConflictSideSchema.nullable(),
  ours: ConflictSideSchema.nullable(),
  theirs: ConflictSideSchema.nullable(),
});

export type ConflictSides = z.infer<typeof ConflictSidesSchema>;

/** A conflicted file: its sides, and what's in the working tree at its path. */
export const ConflictSchema = ConflictSidesSchema.extend({
  /**
   * The file in the working tree as text, conflict markers and all, with its version (see
   * `readWorkingTreeFile`); `null` if it isn't text that can be shown (see `unreadable`).
   */
  text: z.object({ contents: z.string(), version: z.string() }).nullable(),
  /** The version of the file in the working tree, text or not; `null` if there's none there. */
  version: z.string().nullable(),
  /** Whether the file in the working tree is binary, which git leaves as ours without markers. */
  binary: z.boolean(),
  /** Why the file in the working tree isn't `text`, e.g. it's too large; `null` if it is. */
  unreadable: z.string().nullable(),
});

export type Conflict = z.infer<typeof ConflictSchema>;
