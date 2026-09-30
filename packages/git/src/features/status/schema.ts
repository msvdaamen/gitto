import { z } from "zod";

export const FileStatusSchema = z.enum([
  "modified",
  "added",
  "deleted",
  "renamed",
  "copied",
  "typechange",
  "untracked",
  "conflicted",
]);

export type FileStatus = z.infer<typeof FileStatusSchema>;

export const StatusFileSchema = z.object({
  path: z.string(),
  /** Previous path, for renames and copies. */
  origPath: z.string().nullable(),
  /** Change in the index relative to HEAD; `null` when nothing is staged. */
  staged: FileStatusSchema.nullable(),
  /** Change in the working tree relative to the index; `null` when there's nothing unstaged. */
  unstaged: FileStatusSchema.nullable(),
});

export type StatusFile = z.infer<typeof StatusFileSchema>;

export const StatusSchema = z.object({
  /** Current branch name; `null` when HEAD is detached. */
  branch: z.string().nullable(),
  /** HEAD commit; `null` on a branch without commits yet. */
  head: z.string().nullable(),
  upstream: z.string().nullable(),
  ahead: z.number(),
  behind: z.number(),
  files: z.array(StatusFileSchema),
});

export type Status = z.infer<typeof StatusSchema>;
