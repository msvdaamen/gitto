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

export type LineRange = z.infer<typeof LineRangeSchema>;
export type LineSelection = z.infer<typeof LineSelectionSchema>;
