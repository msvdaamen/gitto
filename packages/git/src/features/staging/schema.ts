import * as z from "zod";

/** Lines of one side of a file, by their numbers, from `start` to `end`. */
export const LineRangeSchema = z.object({
  start: z.number().int().min(1),
  end: z.number().int().min(1),
});

/**
 * Lines picked from a file's patch, by their numbers as its hunk headers count them: removed ones
 * in the file's old side, added ones in its new side. Unchanged lines are left out, as there's
 * nothing to stage in them.
 */
export const LineSelectionSchema = z.object({
  deletions: z.array(LineRangeSchema),
  additions: z.array(LineRangeSchema),
});

/** Which side of the uncommitted changes a file's are on: in the working tree, or in the index. */
export const UncommittedSideSchema = z.enum(["unstaged", "staged"]);

/**
 * A change discarding them all kept, as it couldn't discard it: a submodule's or a repository's
 * inside this one, which are discarded in them; a deleted file's something has taken the place of;
 * or an untracked file's it couldn't delete.
 */
export const KeptChangeSchema = z.object({
  path: z.string(),
  reason: z.enum(["submodule", "repository", "in-the-way", "undeletable"]),
});

export type KeptChange = z.infer<typeof KeptChangeSchema>;

export type LineRange = z.infer<typeof LineRangeSchema>;
export type LineSelection = z.infer<typeof LineSelectionSchema>;
export type UncommittedSide = z.infer<typeof UncommittedSideSchema>;
