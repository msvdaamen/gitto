import { z } from "zod";

import { FileStatusSchema } from "../status/schema";

export const ChangedFileSchema = z.object({
  path: z.string(),
  status: FileStatusSchema,
  origPath: z.string().nullable(),
  /** `null` for binary files. */
  additions: z.number().nullable(),
  deletions: z.number().nullable(),
});

export type ChangedFile = z.infer<typeof ChangedFileSchema>;

export const WorkingTreeFilesSchema = z.object({
  /** Changes in the index, compared to HEAD: what the next commit will contain. */
  staged: z.array(ChangedFileSchema),
  /** Changes in the working tree, compared to the index, followed by the untracked files. */
  unstaged: z.array(ChangedFileSchema),
});

export type WorkingTreeFiles = z.infer<typeof WorkingTreeFilesSchema>;
