import { QueryClient } from "@tanstack/solid-query";

import { gitKeys } from "@/git/queries/keys";

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // Git errors (a bad revision, a folder that's no longer a repository) don't go away by retrying.
      retry: false,
      // Calls to Gitto's main process, never paused while the browser thinks it's offline.
      networkMode: "always",
    },
    mutations: { networkMode: "always" },
  },
});

// What's loaded from a repository is refetched when the repository changes on disk (see
// `useRepositoryWatcher`), and only then: not also whenever something that shows it is rendered,
// like the uncommitted changes' row scrolling back into view, which would run `git status` again,
// or when the window is shown again.
queryClient.setQueryDefaults(gitKeys.all, { staleTime: Infinity, refetchOnWindowFocus: false });
