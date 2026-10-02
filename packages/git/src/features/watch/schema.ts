import { z } from "zod";

/**
 * What changed in the git directory: `index` when something was staged or unstaged; `refs` when
 * HEAD, a branch, a tag, the config or an operation in progress (a merge, a rebase) changed.
 */
export const GitDirChangeSchema = z.enum(["index", "refs"]);

export type GitDirChange = z.infer<typeof GitDirChangeSchema>;

/**
 * What the working tree watch reports: `ready` once, when it's watching, as changes before that
 * aren't reported; `changed` whenever a file that shows changed.
 */
export const TreeEventSchema = z.enum(["ready", "changed"]);

export type TreeEvent = z.infer<typeof TreeEventSchema>;
