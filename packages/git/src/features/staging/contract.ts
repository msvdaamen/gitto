import { oc } from "@orpc/contract";
import * as z from "zod";

import { FileInput, RepositoryInput } from "../../input";
import { LineSelectionSchema } from "./schema";

const PathsInput = RepositoryInput.extend({ paths: z.array(z.string().min(1)).min(1) });

/** Lines of a file's changes, picked from `patch`, its patch as it was on show. */
const LinesInput = FileInput.extend({ patch: z.string(), lines: LineSelectionSchema });

/** The file's patch on the same side, once the lines are staged or unstaged. */
const LinesOutput = z.object({ patch: z.string() });

export const StagingContract = {
  stage: oc.input(PathsInput),
  unstage: oc.input(PathsInput),
  /** Stages every change, untracked files and conflicts included. */
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
};
