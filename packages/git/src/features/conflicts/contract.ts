import { oc } from "@orpc/contract";
import * as z from "zod";

import { RepositoryInput } from "../../input";
import { ConflictSchema, ConflictSidesSchema } from "./schema";

const PathInput = RepositoryInput.extend({ path: z.string().min(1) });

export const ConflictsContract = {
  /**
   * A conflicted file: its sides, and the file in the working tree, as text with its conflict
   * markers if it's text. Its sides are all `null` once it isn't conflicted any more.
   */
  get: oc.input(PathInput).output(ConflictSchema),
  /**
   * Resolves a conflicted file by keeping one side of it whole, or its deletion, and marks it
   * resolved. Fails with CONFLICT if the conflict, or the file in the working tree (at `version`,
   * when given), isn't as `sides` had it any more.
   */
  keep: oc.input(
    PathInput.extend({
      side: z.enum(["ours", "theirs"]),
      sides: ConflictSidesSchema,
      version: z.string().nullable(),
    }),
  ),
  /**
   * Marks a conflicted file resolved as it is in the working tree, by staging it. Fails with
   * PRECONDITION_FAILED if it still has conflict markers, unless `withMarkers` (the user said they
   * belong in it), and with CONFLICT if it isn't at `version` (when given) any more.
   */
  markResolved: oc.input(
    PathInput.extend({ version: z.string().nullable(), withMarkers: z.boolean().optional() }),
  ),
};
