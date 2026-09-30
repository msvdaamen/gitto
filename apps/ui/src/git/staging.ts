import type { ChangedFile } from "@gitto/git/types";
import { useMutation, useQueryClient } from "@tanstack/solid-query";

import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";

export function useStage(repositoryId: () => string) {
  const queryClient = useQueryClient();
  return useMutation(() => ({
    mutationFn: (paths: string[]) => rpc.git.staging.stage({ repositoryId: repositoryId(), paths }),
    // The watcher would catch this too, but refetching right away feels snappier.
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: gitKeys.repository(repositoryId()) }),
  }));
}

export function useUnstage(repositoryId: () => string) {
  const queryClient = useQueryClient();
  return useMutation(() => ({
    mutationFn: (paths: string[]) =>
      rpc.git.staging.unstage({ repositoryId: repositoryId(), paths }),
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: gitKeys.repository(repositoryId()) }),
  }));
}

/** Paths to stage or unstage for files; renames need their old path too, for the deletion side. */
export function stagingPaths(files: ChangedFile[]): string[] {
  return files.flatMap((file) => (file.origPath ? [file.path, file.origPath] : [file.path]));
}
