import { z } from "zod";

import { FileStatusSchema } from "../../schema";

export const ChangedFileSchema = z.object({
  path: z.string(),
  status: FileStatusSchema,
  origPath: z.string().nullable(),
  /** `null` for binary files, and for files whose lines weren't counted. */
  additions: z.number().nullable(),
  deletions: z.number().nullable(),
  /** A submodule, a repository of its own committed in this one, whose changes are made in it. */
  submodule: z.literal(true).optional(),
});

export type ChangedFile = z.infer<typeof ChangedFileSchema>;
