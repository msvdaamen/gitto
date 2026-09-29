import { Outlet, createRootRoute } from "@tanstack/solid-router";
import { TanStackRouterDevtools } from "@tanstack/solid-router-devtools";
import { onMount } from "solid-js";

import { rpc } from "@/lib/rpc";

export const Route = createRootRoute({
  component: RootComponent,
});

function RootComponent() {
  // TODO: remove, IPC smoke test.
  onMount(async () => {
    const { message } = await rpc.system.hello({ name: "Gitto" });
    console.log(message);
  });

  return (
    <>
      <Outlet />
      <TanStackRouterDevtools position="bottom-right" />
    </>
  );
}
