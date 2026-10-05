import type { Overview, StatusSummary } from "@gitto/git/types";
import { mapArray } from "solid-js";

import { useOverview } from "@/git/queries/overview";
import { useUnwatchedStatus } from "@/git/queries/status";
import { useUnsuspendedData } from "@/git/queries/unsuspended";
import type { Repository } from "@/hooks/repositories";

/** A repository on the home page, with what's loaded of it so far. */
export interface RepositorySummary {
  repository: Repository;
  /** Where HEAD is, and how many files changed; `undefined` until loaded, or if it couldn't be. */
  readonly status: StatusSummary | undefined;
  readonly overview: Overview | undefined;
  /** Why the repository couldn't be read, e.g. its folder is gone. */
  readonly error: Error | null;
  /** Whether its status is loaded, or couldn't be. */
  readonly settled: boolean;
}

/**
 * The status and overview of every repository, kept up to date while the home page shows them (see
 * `UNWATCHED`). Read without Suspense: each repository shows what it has as it loads, and a refetch
 * doesn't take the page away meanwhile.
 */
export function useRepositorySummaries(
  repositories: () => Repository[],
): () => RepositorySummary[] {
  return mapArray(repositories, (repository) => {
    const status = useUnwatchedStatus(() => repository.id);
    const overview = useOverview(() => repository.id);
    const statusData = useUnsuspendedData(status);
    const overviewData = useUnsuspendedData(overview);
    return {
      repository,
      get status() {
        return statusData();
      },
      get overview() {
        return overviewData();
      },
      get error() {
        return status.error ?? overview.error ?? null;
      },
      get settled() {
        return statusData() !== undefined || status.isError;
      },
    };
  });
}
