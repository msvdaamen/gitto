import type { ChangedFile } from "@gitto/git/types";
import { useMutation, useQueryClient } from "@tanstack/solid-query";

import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";

/** Files to stage or unstage: their paths (see `stagingPaths`), or all of them. */
export type StagingTarget = string[] | "all";

export function useStage(repositoryId: () => string) {
  const queryClient = useQueryClient();
  return useMutation(() => ({
    mutationFn: (target: StagingTarget) => {
      const id = repositoryId();
      return target === "all"
        ? rpc.git.staging.stageAll({ repositoryId: id })
        : rpc.git.staging.stage({ repositoryId: id, paths: target });
    },
    // The watcher would catch this too, but refetching right away feels snappier.
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: gitKeys.uncommitted(repositoryId()) }),
  }));
}

export function useUnstage(repositoryId: () => string) {
  const queryClient = useQueryClient();
  return useMutation(() => ({
    mutationFn: (target: StagingTarget) => {
      const id = repositoryId();
      return target === "all"
        ? rpc.git.staging.unstageAll({ repositoryId: id })
        : rpc.git.staging.unstage({ repositoryId: id, paths: target });
    },
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: gitKeys.uncommitted(repositoryId()) }),
  }));
}

/** Paths to stage or unstage for files; renames need their old path too, for the deletion side. */
export function stagingPaths(files: ChangedFile[]): string[] {
  return files.flatMap((file) => (file.origPath ? [file.path, file.origPath] : [file.path]));
}
