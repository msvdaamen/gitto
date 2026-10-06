import type { ChangedFile, KeptChange, LineSelection, UncommittedSide } from "@gitto/git/types";
import { hashKey, useMutation, useQueryClient } from "@tanstack/solid-query";

import { stagingPaths } from "@/git/changes";
import { hasHunks } from "@/git/patch";
import { rpc } from "@/lib/rpc";

import type { FilePatch } from "./file-diff";
import { gitKeys } from "./keys";

/** Files to stage or unstage: their paths (see `stagingPaths`), or all of them. */
export type StagingTarget = string[] | "all";

export function useStage(repositoryId: () => string) {
  return useStagingMutation(repositoryId, (id, target: StagingTarget) =>
    target === "all"
      ? rpc.git.staging.stageAll({ repositoryId: id })
      : rpc.git.staging.stage({ repositoryId: id, paths: target }),
  );
}

export function useUnstage(repositoryId: () => string) {
  return useStagingMutation(repositoryId, (id, target: StagingTarget) =>
    target === "all"
      ? rpc.git.staging.unstageAll({ repositoryId: id })
      : rpc.git.staging.unstage({ repositoryId: id, paths: target }),
  );
}

/** Changes to discard: a file's, on the side of the uncommitted changes it's listed on, or all. */
export type DiscardTarget = { file: ChangedFile; side: UncommittedSide } | "all";

/**
 * Discards changes, which can't be brought back: a file's (see `discard` in the git package), or
 * all of them, untracked files included. Resolves to the paths of the changes kept, as they can't
 * be discarded (see `discardAll`).
 */
export function useDiscard(repositoryId: () => string) {
  return useStagingMutation(
    repositoryId,
    async (id, target: DiscardTarget): Promise<KeptChange[]> => {
      if (target === "all") return (await rpc.git.staging.discardAll({ repositoryId: id })).kept;
      const { file, side } = target;
      await rpc.git.staging.discard({
        repositoryId: id,
        path: file.path,
        origPath: file.origPath,
        // Checked to still be its status: its list may be out of date.
        status: file.status,
        side,
      });
      return [];
    },
  );
}

function useStagingMutation<T, R = unknown>(
  repositoryId: () => string,
  run: (repositoryId: string, target: T) => Promise<R>,
) {
  const queryClient = useQueryClient();
  return useMutation(() => ({
    // The repository it runs on, kept for when it's done: another may be on show by then.
    onMutate: () => ({ id: repositoryId() }),
    mutationFn: (target: T) => run(repositoryId(), target),
    // The watcher would catch this too, but refetching right away feels snappier. Also after a
    // failure, which may have changed some of it.
    onSettled: (_data, _error, _target, started) =>
      queryClient.invalidateQueries({
        queryKey: gitKeys.uncommitted(started?.id ?? repositoryId()),
      }),
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
