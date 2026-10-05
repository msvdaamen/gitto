import { oc } from "@orpc/contract";
import * as z from "zod";

import { FullSha, RepositoryInput, Sha } from "../../input";
import { ChangedFileSchema } from "./schema";

export const DiffContract = {
  /** Files changed by a commit, compared to its first parent. */
  commitFiles: oc.input(RepositoryInput.extend({ sha: Sha })).output(z.array(ChangedFileSchema)),
  /** The patch of one file a commit changed, compared to its first parent. */
  commitFilePatch: oc
    .input(RepositoryInput.extend({ sha: Sha, path: z.string(), origPath: z.string().nullable() }))
    .output(z.string()),
  /** A file's contents, by its object name, e.g. from a patch's `index` line. */
  blob: oc.input(RepositoryInput.extend({ oid: FullSha })).output(z.string()),
};
