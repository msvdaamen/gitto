import type { ChangedFile, Status } from "@gitto/git/types";
import { keepPreviousData, useQuery } from "@tanstack/solid-query";
import { createMemo } from "solid-js";

import { rpc } from "@/lib/rpc";
import type { Commit } from "@/types/git";

import { gitKeys } from "./keys";
import { useStatus } from "./status";

/** Files changed by a history row: a commit's files, or the uncommitted changes. */
export function useChangedFiles(commit: () => Commit) {
  const query = useQuery(() => {
    const { repositoryId, id, isWip } = commit();
    return {
      queryKey: isWip
        ? gitKeys.workingTreeFiles(repositoryId)
        : gitKeys.commitFiles(repositoryId, id),
      queryFn: ({ signal }: { signal: AbortSignal }) =>
        isWip
          ? rpc.git.diff.workingTreeFiles({ repositoryId }, { signal })
          : rpc.git.diff.commitFiles({ repositoryId, sha: id }, { signal }),
      staleTime: isWip ? 0 : Infinity,
      // Keep showing the previous selection's files while the next ones load, instead of suspending.
      placeholderData: keepPreviousData,
    };
  });
  const status = useStatus(() => commit().repositoryId);

  const files = createMemo(() => {
    const data = query.data ?? [];
    return commit().isWip ? withWorkingTreeStatus(data, status.data) : data;
  });
  const totals = createMemo(() =>
    files().reduce(
      (sum, file) => ({
        additions: sum.additions + (file.additions ?? 0),
        deletions: sum.deletions + (file.deletions ?? 0),
      }),
      { additions: 0, deletions: 0 },
    ),
  );

  return { query, files, totals };
}

/**
 * Adds what `git diff HEAD` leaves out of the working tree files: untracked files, files that were
 * staged and then deleted (in neither HEAD nor the working tree, but still about to be committed),
 * and which files are conflicted.
 */
function withWorkingTreeStatus(files: ChangedFile[], status: Status | undefined): ChangedFile[] {
  if (!status) return files;

  const conflicted = new Set(
    status.files.filter((file) => file.staged === "conflicted").map((file) => file.path),
  );
  const diffed = new Set(files.map((file) => file.path));
  const missing = status.files
    .filter((file) => !diffed.has(file.path))
    .map<ChangedFile>((file) => ({
      path: file.path,
      origPath: file.origPath,
      status:
        file.unstaged === "untracked" ? "untracked" : (file.staged ?? file.unstaged ?? "modified"),
      additions: null,
      deletions: null,
    }));

  return [
    ...files.map((file) =>
      conflicted.has(file.path) ? { ...file, status: "conflicted" as const } : file,
    ),
    ...missing,
  ];
}
