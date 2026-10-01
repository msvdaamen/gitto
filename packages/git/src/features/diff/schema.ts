import { z } from "zod";

import { FileStatusSchema } from "../../schema";

export const ChangedFileSchema = z.object({
  path: z.string(),
  status: FileStatusSchema,
  origPath: z.string().nullable(),
  /** `null` for binary files. */
  additions: z.number().nullable(),
  deletions: z.number().nullable(),
});

export type ChangedFile = z.infer<typeof ChangedFileSchema>;
