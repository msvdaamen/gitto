// Renders the whole app, on the fake repository's page, the way index.tsx does in Electron.
import "@/index.css";
import { QueryClientProvider } from "@tanstack/solid-query";
import { createMemoryHistory, createRouter, RouterProvider } from "@tanstack/solid-router";
import { render } from "solid-js/web";
import { expect } from "vitest";
import { page } from "vitest/browser";

import { queryClient } from "@/lib/query-client";
import { routeTree } from "@/routeTree.gen";

import { quiet } from "./frames";
import { REPOSITORY } from "./rpc";

/** Opens the repository page, and resolves once its history is on show. Returns its unmounting. */
export async function renderApp(): Promise<() => void> {
  const router = createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: [`/${REPOSITORY.id}`] }),
  });
  const root = document.createElement("div");
  root.id = "root";
  document.body.append(root);
  const dispose = render(
    () => (
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    ),
    root,
  );
  const history = page.getByRole("listbox", { name: "Commit history" });
  await expect.element(history.getByRole("option").first()).toBeVisible();
  await quiet();
  return () => {
    dispose();
    root.remove();
    queryClient.clear();
  };
}
