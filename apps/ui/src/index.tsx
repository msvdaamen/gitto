/* @refresh reload */
import "./index.css";
import { QueryClientProvider } from "@tanstack/solid-query";
import { RouterProvider, createRouter } from "@tanstack/solid-router";
import { render } from "solid-js/web";

import { queryClient } from "./lib/query-client";
import { traceLongFrames } from "./lib/trace";
import { routeTree } from "./routeTree.gen";

if (import.meta.env.DEV) traceLongFrames();

const router = createRouter({
  routeTree,
  defaultPreload: "intent",
  scrollRestoration: true,
});

// Register things for typesafety
declare module "@tanstack/solid-router" {
  interface Register {
    router: typeof router;
  }
}

const rootElement = document.getElementById("root")!;

if (!rootElement.innerHTML) {
  render(
    () => (
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    ),
    rootElement,
  );
}
