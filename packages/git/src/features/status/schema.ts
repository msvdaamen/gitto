import { z } from "zod";

import { FileStatusSchema } from "../../schema";
import { ChangedFileSchema } from "../diff/schema";

export const StatusFileSchema = z.object({
  path: z.string(),
  /** Previous path, for renames and copies. */
  origPath: z.string().nullable(),
  /** Change in the index relative to HEAD; `null` when nothing is staged. */
  staged: FileStatusSchema.nullable(),
  /** Change in the working tree relative to the index; `null` when there's nothing unstaged. */
  unstaged: FileStatusSchema.nullable(),
  /**
   * How a conflicted file conflicts, as `git status` says: `UU` both modified, `AA` both added,
   * `UD` and `DU` deleted by them and by us, `AU` and `UA` added by us and by them only, `DD`
   * deleted by both; with the modes ours and theirs have (`000000` where it's not there).
   */
  conflict: z.object({ xy: z.string(), ours: z.string(), theirs: z.string() }).optional(),
  /** A submodule, a repository of its own committed in this one. */
  submodule: z.literal(true).optional(),
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

export const WorkingTreeFilesSchema = z.object({
  /** Changes in the index, compared to HEAD: what the next commit will contain. */
  staged: z.array(ChangedFileSchema),
  /** Changes in the working tree, compared to the index, followed by the untracked files. */
  unstaged: z.array(ChangedFileSchema),
  /**
   * Whether the files' lines weren't counted, as there are too many to diff on every change: none
   * has line counts then, binary or not.
   */
  uncounted: z.boolean(),
  /**
   * The conflicted text files, changed or added on both sides, that have no conflict markers left:
   * resolved in the working tree, but not marked resolved (staged) yet.
   */
  markerFree: z.array(z.string()),
});

export type WorkingTreeFiles = z.infer<typeof WorkingTreeFilesSchema>;

/**
 * How many files changed. A conflict isn't counted as staged or unstaged, though git reports it on
 * both sides; `files` counts each changed file once.
 */
export const StatusCountsSchema = z.object({
  files: z.number(),
  staged: z.number(),
  unstaged: z.number(),
  conflicted: z.number(),
});

export type StatusCounts = z.infer<typeof StatusCountsSchema>;

/** Where HEAD is, and how much changed: what the UI shows outside the list of changed files. */
export const StatusSummarySchema = StatusSchema.omit({ files: true }).extend({
  counts: StatusCountsSchema,
});

export type StatusSummary = z.infer<typeof StatusSummarySchema>;

/** Everything uncommitted: the summary and the changed files, with their line counts if any. */
export const UncommittedSchema = StatusSummarySchema.extend({
  changes: WorkingTreeFilesSchema,
  /** Changes whenever anything above does, so a caller can ask to skip a status it already has. */
  version: z.string(),
});

export type Uncommitted = z.infer<typeof UncommittedSchema>;
