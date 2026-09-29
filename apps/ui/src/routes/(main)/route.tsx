import { createFileRoute, Outlet } from "@tanstack/solid-router";

import { Footer } from "@/components/footer";
import { Header } from "@/components/header";

export const Route = createFileRoute("/(main)")({
  component: RouteComponent,
});

function RouteComponent() {
  return (
    <div class="grid h-screen grid-rows-[49px_minmax(0,1fr)_27px] bg-bg">
      <Header />
      <div class="min-h-0 min-w-0 overflow-hidden">
        <Outlet />
      </div>
      <Footer />
    </div>
  );
}
