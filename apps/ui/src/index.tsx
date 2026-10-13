/* @refresh reload */
import "./index.css";
import { captureException, initTelemetry, traceStartup } from "@gitto/telemetry-ui";
import { QueryClientProvider } from "@tanstack/solid-query";
import { RouterProvider, createRouter } from "@tanstack/solid-router";
import { render } from "solid-js/web";

import { queryClient } from "./lib/query-client";
import { SCROLL_TO_TOP } from "./lib/scroll";
import { traceLongFrames } from "./lib/trace";
import { routeTree } from "./routeTree.gen";

// Before anything else, to catch its errors and time it.
initTelemetry({ enabled: import.meta.env.SENTRY });

if (import.meta.env.DEV) traceLongFrames();

const router = createRouter({
  routeTree,
  defaultPreload: "intent",
  scrollRestoration: true,
  scrollToTopSelectors: SCROLL_TO_TOP,
  defaultOnCatch: (error) => captureException(error),
});

// Register things for typesafety
declare module "@tanstack/solid-router" {
  interface Register {
    router: typeof router;
  }
}

const rootElement = document.getElementById("root")!;

if (!rootElement.innerHTML) {
  traceStartup("render", "ui.render", () =>
    render(
      () => (
        <QueryClientProvider client={queryClient}>
          <RouterProvider router={router} />
        </QueryClientProvider>
      ),
      rootElement,
    ),
  );
}
