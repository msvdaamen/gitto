import { Outlet, createRootRoute } from "@tanstack/solid-router";
import { TanStackRouterDevtools } from "@tanstack/solid-router-devtools";

import { GitVersionDialog } from "@/components/git-version-dialog";
import { WhatsNewDialog } from "@/components/whats-new-dialog";

export const Route = createRootRoute({
  component: RootComponent,
});

function RootComponent() {
  return (
    <>
      <Outlet />
      <GitVersionDialog />
      <WhatsNewDialog />
      <TanStackRouterDevtools position="bottom-right" />
    </>
  );
}
