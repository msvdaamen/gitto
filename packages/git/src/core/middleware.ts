import { ORPCError, os } from "@orpc/server";

import {
  ChangesStashedError,
  ChangesTooLargeError,
  ConflictChangedError,
  ConflictMarkersError,
  DiscardBlockedError,
  EditorNeededError,
  FileChangedOnDiskError,
  FileTooLargeError,
  FolderNotFoundError,
  GitError,
  HeadMovedError,
  IndexLockedError,
  LinesNotStageableError,
  MergeBlockedError,
  MergeStoppedError,
  NoOperationError,
  NoUpstreamError,
  NotARepositoryError,
  NotUtf8Error,
  OutsideRepositoryError,
  PatchChangedError,
  PullInterruptedError,
  RepositoryChangedError,
  RepositoryNotFoundError,
  StashConflictError,
  UnsupportedGitError,
  WorkingTreeFileNotFoundError,
} from "./errors";
import type { GitRepos } from "./repo";
import type { GitVersion } from "./version";

export interface GitContext {
  gitRepos: GitRepos;
  gitVersion: GitVersion;
}

/**
 * Opens the repository named by the procedure's `repositoryId` input as `context.repo`, and turns
 * the git package's errors into ones the renderer can show. Nothing runs with a git Gitto doesn't
 * support: its commands would fail partway, or be misread, rather than say why.
 */
export const withRepo = os
  .$context<GitContext>()
  .middleware(async ({ context, next }, input: { repositoryId: string }) => {
    try {
      await context.gitVersion.require();
      return await next({ context: { repo: await context.gitRepos.open(input.repositoryId) } });
    } catch (error) {
      throw toApiError(error);
    }
  });

/** The one place that decides how each git error reaches the renderer. */
function toApiError(error: unknown): unknown {
  if (
    error instanceof RepositoryNotFoundError ||
    error instanceof FolderNotFoundError ||
    error instanceof WorkingTreeFileNotFoundError ||
    error instanceof NotARepositoryError
  ) {
    return new ORPCError("NOT_FOUND", { message: error.message, cause: error });
  }
  if (error instanceof FileTooLargeError || error instanceof ChangesTooLargeError) {
    return new ORPCError("PAYLOAD_TOO_LARGE", { message: error.message, cause: error });
  }
  if (error instanceof OutsideRepositoryError) {
    return new ORPCError("FORBIDDEN", { message: error.message, cause: error });
  }
  if (error instanceof NotUtf8Error) {
    return new ORPCError("UNSUPPORTED_MEDIA_TYPE", { message: error.message, cause: error });
  }
  if (error instanceof LinesNotStageableError) {
    return new ORPCError("UNPROCESSABLE_CONTENT", { message: error.message, cause: error });
  }
  if (
    error instanceof NoUpstreamError ||
    error instanceof UnsupportedGitError ||
    error instanceof ConflictMarkersError ||
    error instanceof NoOperationError ||
    error instanceof EditorNeededError ||
    error instanceof MergeBlockedError ||
    error instanceof DiscardBlockedError
  ) {
    return new ORPCError("PRECONDITION_FAILED", { message: error.message, cause: error });
  }
  if (
    error instanceof IndexLockedError ||
    error instanceof HeadMovedError ||
    error instanceof ChangesStashedError ||
    error instanceof PullInterruptedError ||
    error instanceof MergeStoppedError ||
    error instanceof RepositoryChangedError ||
    error instanceof StashConflictError ||
    error instanceof FileChangedOnDiskError ||
    error instanceof PatchChangedError ||
    error instanceof ConflictChangedError
  ) {
    return new ORPCError("CONFLICT", { message: error.message, cause: error });
  }
  if (error instanceof GitError) {
    return new ORPCError("INTERNAL_SERVER_ERROR", { message: error.message, cause: error });
  }
  return error;
}
