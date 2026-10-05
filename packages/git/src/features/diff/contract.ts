import { oc } from "@orpc/contract";
import * as z from "zod";

import { FullSha, RepositoryInput, Sha } from "../../input";
import { ChangedFileSchema } from "./schema";

/** A file, by its path in the repository and, for a rename, its previous one. */
const FileInput = RepositoryInput.extend({ path: z.string(), origPath: z.string().nullable() });

export const DiffContract = {
  /** Files changed by a commit, compared to its first parent. */
  commitFiles: oc.input(RepositoryInput.extend({ sha: Sha })).output(z.array(ChangedFileSchema)),
  /** The patch of one file a commit changed, compared to its first parent. */
  commitFilePatch: oc.input(FileInput.extend({ sha: Sha })).output(z.string()),
  /**
   * The patch of one file's unstaged changes, compared to the index; an untracked file's compared
   * to nothing. Empty if it has none (any more).
   */
  unstagedFilePatch: oc.input(FileInput.extend({ untracked: z.boolean() })).output(z.string()),
  /** The patch of one file's staged changes, compared to HEAD. Empty if it has none (any more). */
  stagedFilePatch: oc.input(FileInput).output(z.string()),
  /** A file's contents, by its object name, e.g. from a patch's `index` line. */
  blob: oc.input(RepositoryInput.extend({ oid: FullSha })).output(z.string()),
  /** A file's contents in the working tree, which can change at any time. */
  workingTreeFile: oc.input(RepositoryInput.extend({ path: z.string() })).output(z.string()),
};
