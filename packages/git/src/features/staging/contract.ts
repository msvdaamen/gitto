import { oc } from "@orpc/contract";
import * as z from "zod";

import { FileInput, RepositoryInput } from "../../input";
import { KeptChangeSchema, LineSelectionSchema, UncommittedSideSchema } from "./schema";

const PathsInput = RepositoryInput.extend({ paths: z.array(z.string().min(1)).min(1) });

/** Lines of a file's changes, picked from `patch`, its patch as it was on show. */
const LinesInput = FileInput.extend({ patch: z.string(), lines: LineSelectionSchema });

/** The file's patch on the same side, once the lines are staged or unstaged. */
const LinesOutput = z.object({ patch: z.string() });

export const StagingContract = {
  /**
   * Stages files whole; fails with PRECONDITION_FAILED, staging none, if a conflicted one still has
   * conflict markers, as that would mark it resolved with them.
   */
  stage: oc.input(PathsInput),
  unstage: oc.input(PathsInput),
  /** Stages every change, untracked files and conflicts included; see `stage` for the conflicts. */
  stageAll: oc.input(RepositoryInput),
  /** Unstages every staged change; conflicts stay conflicted. */
  unstageAll: oc.input(RepositoryInput),
  /**
   * Stages some lines of a file's unstaged changes; fails with CONFLICT if `patch` isn't its
   * unstaged changes' patch any more, and with UNPROCESSABLE_CONTENT if they can't be staged by
   * line, e.g. as the file is binary. Returns the unstaged changes' patch they leave.
   */
  stageLines: oc.input(LinesInput.extend({ untracked: z.boolean() })).output(LinesOutput),
  /** Unstages some lines of a file's staged changes, returning the staged ones left; see `stageLines`. */
  unstageLines: oc.input(LinesInput).output(LinesOutput),
  /**
   * Discards a file's changes on `side`, the side it's listed on: its unstaged ones, or all of them
   * from its staged ones on (see `discard`). `origPath` is a rename's previous path, which is put
   * back too; not a copy's source, which is another file. Fails with PRECONDITION_FAILED,
   * discarding nothing, if it's conflicted.
   */
  discard: oc.input(
    // Not an empty path, which, as a pathspec, names every file.
    FileInput.extend({ path: z.string().min(1), side: UncommittedSideSchema }),
  ),
  /**
   * Discards every uncommitted change, untracked files included; fails with PRECONDITION_FAILED,
   * discarding nothing, while files are conflicted or an operation like a merge is under way.
   * Returns the changes it kept, as it can't discard them, and why (see `discardAll`).
   */
  discardAll: oc.input(RepositoryInput).output(z.object({ kept: z.array(KeptChangeSchema) })),
};
