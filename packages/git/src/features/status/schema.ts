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

/** What HEAD points at. */
export const HeadSchema = z.discriminatedUnion("kind", [
  /** A branch, at commit `sha`. */
  z.object({ kind: z.literal("branch"), name: z.string(), sha: z.string() }),
  /** A branch without commits yet, e.g. in a new repository. */
  z.object({ kind: z.literal("unborn"), name: z.string() }),
  /** A commit, with no branch checked out. */
  z.object({ kind: z.literal("detached"), sha: z.string() }),
]);

export type Head = z.infer<typeof HeadSchema>;

export const StatusSchema = z.object({
  head: HeadSchema,
  upstream: z.string().nullable(),
  ahead: z.number(),
  behind: z.number(),
  files: z.array(StatusFileSchema),
});

export type Status = z.infer<typeof StatusSchema>;
