import { useQuery } from "@tanstack/solid-query";
import { createMemo } from "solid-js";

import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";
import { toCommitRows } from "./rows";
import { useStatus } from "./status";

export function useLog(repositoryId: () => string) {
  return useQuery(() => {
    const id = repositoryId();
    return {
      queryKey: gitKeys.log(id),
      // Tagged with its repository, so every row knows where its commit lives (see `toCommitRows`).
      queryFn: async ({ signal }) => ({
        repositoryId: id,
        commits: await rpc.git.history.log({ repositoryId: id }, { signal }),
      }),
    };
  });
}

/**
 * History table rows: the uncommitted changes (if any) followed by the log. `selectedId` picks the
 * selected row; it defaults to the first one.
 */
export function useHistory(repositoryId: () => string, selectedId: () => string | undefined) {
  const log = useLog(repositoryId);
  const status = useStatus(repositoryId);

  const rows = createMemo(() =>
    log.data ? toCommitRows(log.data.repositoryId, log.data.commits, status.data) : [],
  );
  const selected = createMemo(() => rows().find((row) => row.id === selectedId()) ?? rows()[0]);

  return { log, rows, selected };
}
