import type { ChangedFile, LineSelection } from "@gitto/git/types";
import { hashKey, useMutation, useQueryClient } from "@tanstack/solid-query";

import { stagingPaths } from "@/git/changes";
import { rpc } from "@/lib/rpc";

import type { FilePatch } from "./file-diff";
import { gitKeys } from "./keys";

/** Files to stage or unstage: their paths (see `stagingPaths`), or all of them. */
export type StagingTarget = string[] | "all";

export function useStage(repositoryId: () => string) {
  return useStagingMutation(repositoryId, (id, target) =>
    target === "all"
      ? rpc.git.staging.stageAll({ repositoryId: id })
      : rpc.git.staging.stage({ repositoryId: id, paths: target }),
  );
}

export function useUnstage(repositoryId: () => string) {
  return useStagingMutation(repositoryId, (id, target) =>
    target === "all"
      ? rpc.git.staging.unstageAll({ repositoryId: id })
      : rpc.git.staging.unstage({ repositoryId: id, paths: target }),
  );
}

function useStagingMutation(
  repositoryId: () => string,
  run: (repositoryId: string, target: StagingTarget) => Promise<unknown>,
) {
  const queryClient = useQueryClient();
  return useMutation(() => ({
    mutationFn: (target: StagingTarget) => run(repositoryId(), target),
    // The watcher would catch this too, but refetching right away feels snappier.
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: gitKeys.uncommitted(repositoryId()) }),
  }));
}

/** A file to stage or unstage whole. */
export interface FileToStage {
  repositoryId: string;
  action: "stage" | "unstage";
  file: ChangedFile;
}

/**
 * Stages or unstages a whole file: a renamed one by both its paths. Settles once that's done,
 * rather than once the uncommitted changes are refetched after, so the view can move on at once.
 */
export function useStageFile() {
  const queryClient = useQueryClient();
  return useMutation(() => ({
    mutationFn: ({ repositoryId, action, file }: FileToStage) => {
      const input = { repositoryId, paths: stagingPaths([file]) };
      return action === "stage" ? rpc.git.staging.stage(input) : rpc.git.staging.unstage(input);
    },
    onSettled: (_result, _error, { repositoryId }) =>
      void queryClient.invalidateQueries({ queryKey: gitKeys.uncommitted(repositoryId) }),
  }));
}

/** Lines of a file's changes to stage or unstage, picked from `patch`, its patch on show. */
export interface LinesToStage {
  repositoryId: string;
  action: "stage" | "unstage";
  file: ChangedFile;
  patch: string;
  lines: LineSelection;
}

/**
 * Stages or unstages lines of a file's changes; resolves to the file's patch they leave. That takes
 * the place of the one on show at once, if it has changes left: one without is left for the view
 * to move on from, rather than said to have none (see `hasHunks`). The rest of the uncommitted
 * changes, like the lists, are refetched without waiting for them. If the lines couldn't be staged,
 * it settles once all of it is refetched: the patch they were picked from may be out of date, and
 * isn't picked from again until it isn't.
 */
export function useStageLines() {
  const queryClient = useQueryClient();
  return useMutation(() => ({
    mutationFn: async ({ repositoryId, action, file, patch, lines }: LinesToStage) => {
      const input = { repositoryId, path: file.path, origPath: file.origPath, patch, lines };
      const left =
        action === "stage"
          ? await rpc.git.staging.stageLines({ ...input, untracked: file.status === "untracked" })
          : await rpc.git.staging.unstageLines(input);
      return left.patch;
    },
    onSuccess: async (patch, { repositoryId, action, file }) => {
      const side = action === "stage" ? "unstaged" : "staged";
      const key = gitKeys.uncommittedFilePatch(repositoryId, side, file.path);
      // A refetch under way, e.g. the watcher's, could be of the patch from before.
      await queryClient.cancelQueries({ queryKey: key, exact: true });
      const shown =
        hasHunks(patch) &&
        queryClient.setQueryData<FilePatch>(key, (last) => last && { ...last, patch });
      void queryClient.invalidateQueries({
        queryKey: gitKeys.uncommitted(repositoryId),
        predicate: (query) => !shown || hashKey(query.queryKey) !== hashKey(key),
      });
    },
    onError: (_error, { repositoryId }) =>
      queryClient.invalidateQueries({ queryKey: gitKeys.uncommitted(repositoryId) }),
  }));
}

/** Whether `patch` has lines to stage or unstage left, rather than none, or a mode change. */
export function hasHunks(patch: string): boolean {
  return patch.includes("\n@@ ");
}
