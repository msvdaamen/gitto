import { oc } from "@orpc/contract";
import * as z from "zod";

import { FileInput, RepositoryInput } from "../../input";
import { FileStatusSchema } from "../../schema";
import { LineSelectionSchema } from "./schema";

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
   * Discards a file's changes on one side: its unstaged ones put it back as the index has it,
   * deleting it if it's untracked; its staged ones, as HEAD has it, unstaged ones and all. `status`
   * is the file's as its list had it. Fails, discarding nothing: with PRECONDITION_FAILED if it's
   * conflicted, a submodule or a repository inside this one, or if putting it back would lose
   * another file; with CONFLICT if it changed since, so that what's deleted isn't what its status
   * said.
   */
  discard: oc.input(
    FileInput.extend({ side: z.enum(["staged", "unstaged"]), status: FileStatusSchema }),
  ),
  /**
   * Discards every change, deleting untracked files but not ignored ones, and keeping submodules'
   * changes; fails with PRECONDITION_FAILED, discarding nothing, while an operation or conflicts
   * are under way, or if there's nothing else to discard.
   */
  discardAll: oc.input(RepositoryInput),
  /**
   * Stages some lines of a file's unstaged changes; fails with CONFLICT if `patch` isn't its
   * unstaged changes' patch any more, and with UNPROCESSABLE_CONTENT if they can't be staged by
   * line, e.g. as the file is binary. Returns the unstaged changes' patch they leave.
   */
  stageLines: oc.input(LinesInput.extend({ untracked: z.boolean() })).output(LinesOutput),
  /** Unstages some lines of a file's staged changes, returning the staged ones left; see `stageLines`. */
  unstageLines: oc.input(LinesInput).output(LinesOutput),
};
