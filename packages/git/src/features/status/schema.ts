import { z } from "zod";

import { FileStatusSchema } from "../../schema";
import { FileChangeSchema } from "../diff/schema";

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

/**
 * The changed files, without line counts: counting lines means diffing every file, so they're
 * asked for separately, for the files on show (see `lineCounts` in the diff contract).
 */
export const WorkingTreeFilesSchema = z.object({
  /** Changes in the index, compared to HEAD: what the next commit will contain. */
  staged: z.array(FileChangeSchema),
  /** Changes in the working tree, compared to the index, followed by the untracked files. */
  unstaged: z.array(FileChangeSchema),
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

/** Everything uncommitted: the summary and the changed files. */
export const UncommittedSchema = StatusSummarySchema.extend({
  changes: WorkingTreeFilesSchema,
  /** Changes whenever anything above does, so a caller can ask to skip a status it already has. */
  version: z.string(),
});

export type Uncommitted = z.infer<typeof UncommittedSchema>;
