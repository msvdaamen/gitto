import { z } from "zod";

import { FileStatusSchema } from "../../schema";

/** A file that changed, and how. */
export const FileChangeSchema = z.object({
  path: z.string(),
  status: FileStatusSchema,
  /** Previous path, for renames and copies. */
  origPath: z.string().nullable(),
});

export type FileChange = z.infer<typeof FileChangeSchema>;

/** How many lines changed in a file; both `null` for binary files. */
export const LineCountsSchema = z.object({
  path: z.string(),
  additions: z.number().nullable(),
  deletions: z.number().nullable(),
});

export type LineCounts = z.infer<typeof LineCountsSchema>;

/** A file that changed, with its line counts. */
export const ChangedFileSchema = FileChangeSchema.extend(LineCountsSchema.shape);

export type ChangedFile = z.infer<typeof ChangedFileSchema>;
