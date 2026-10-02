import { QueryClient } from "@tanstack/solid-query";

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
