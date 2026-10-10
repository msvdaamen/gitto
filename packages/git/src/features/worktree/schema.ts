import { z } from "zod";

/** A worktree of the repository: the main one, or a linked one checked out in a folder of its own. */
export const WorktreeSchema = z.object({
  /** The folder's absolute path, as git lists it. */
  path: z.string(),
  /** The folder's name, e.g. `gitto-feature`, which the UI calls the worktree by. */
  name: z.string(),
  /** The commit HEAD points at; `null` in a bare one, which has none. */
  head: z.string().nullable(),
  /** The full name of the branch checked out there, e.g. `refs/heads/main`; `null` when detached. */
  branch: z.string().nullable(),
  /** Whether it's the main worktree, whose git directory the others share: it can't be removed. */
  main: z.boolean(),
  /** Whether it's bare: the main one of a bare repository, with no files to open. */
  bare: z.boolean(),
  /** Whether it's the one the repository was opened as, so the one on show. */
  current: z.boolean(),
  /**
   * Why it's locked, which keeps it from being pruned or removed; `""` without a reason, `null`
   * if it isn't.
   */
  locked: z.string().nullable(),
  /** Why it can be pruned, e.g. its folder is gone; `null` if it can't. */
  prunable: z.string().nullable(),
});

export type Worktree = z.infer<typeof WorktreeSchema>;
