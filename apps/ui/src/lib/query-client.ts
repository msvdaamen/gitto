import { QueryClient } from "@tanstack/solid-query";

/** A query client with the app's defaults; tests make their own. */
export const createQueryClient = () =>
  new QueryClient({
    defaultOptions: {
      queries: {
        // Git errors (a bad revision, a folder that's no longer a repository) don't go away by retrying.
        retry: false,
        // What's loaded stays right until something says it changed: the watchers for git data (see
        // `useRepositoryWatcher`), mutations for the rest. Otherwise every component that mounts
        // loads its data again, like the whole status each time the uncommitted changes are selected,
        // and so does the window whenever it's shown.
        staleTime: Infinity,
        refetchOnWindowFocus: false,
      },
    },
  });

export const queryClient = createQueryClient();
