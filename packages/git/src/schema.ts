import { z } from "zod";

/** How a file changed; shared by the status and diffs. */
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
