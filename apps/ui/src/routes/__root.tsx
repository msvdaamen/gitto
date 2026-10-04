import { Outlet, createRootRoute } from "@tanstack/solid-router";
import { TanStackRouterDevtools } from "@tanstack/solid-router-devtools";

import { GitVersionDialog } from "@/components/git-version-dialog";

export const Route = createRootRoute({
  component: RootComponent,
});

function RootComponent() {
  return (
    <>
      <Outlet />
      <GitVersionDialog />
      <TanStackRouterDevtools position="bottom-right" />
    </>
  );
}
