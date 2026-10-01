import { createFileRoute } from "@tanstack/solid-router";
import { cn } from "cn";
import { createSignal } from "solid-js";

import { CommitDetails } from "@/components/repository/commit-details";
import { HistoryTable } from "@/components/repository/history-table";
import { RefsSidebar } from "@/components/repository/refs-sidebar";
import { RepositoryToolbar } from "@/components/repository/toolbar";
import { useRepositoryWatcher } from "@/git/watch";

export const Route = createFileRoute("/(main)/$repoId")({
  component: RouteComponent,
});

function RouteComponent() {
  const params = Route.useParams();
  const repositoryId = () => params().repoId;
  useRepositoryWatcher(repositoryId);

  const [search, setSearch] = createSignal("");
  // Tagged with its repository: the route component is reused when switching repositories, and the
  // new one starts without a selection.
  const [selection, setSelection] = createSignal<{ repositoryId: string; id: string }>();
  const selectedId = () => {
    const current = selection();
    return current?.repositoryId === repositoryId() ? current.id : undefined;
  };
  const [sidebarOpen, setSidebarOpen] = createSignal(true);
  const [detailsOpen, setDetailsOpen] = createSignal(true);

  return (
    <div class="grid h-full grid-rows-[49px_minmax(0,1fr)] overflow-hidden">
      <RepositoryToolbar
        repositoryId={repositoryId()}
        search={search()}
        onSearch={setSearch}
        sidebarOpen={sidebarOpen()}
        onToggleSidebar={() => setSidebarOpen((value) => !value)}
        detailsOpen={detailsOpen()}
        onToggleDetails={() => setDetailsOpen((value) => !value)}
      />

      <div
        class={cn(
          "relative grid min-h-0 min-w-0 overflow-hidden",
          sidebarOpen()
            ? detailsOpen()
              ? "grid-cols-[220px_minmax(0,1fr)_326px] max-lg:grid-cols-[210px_minmax(0,1fr)] max-md:grid-cols-[52px_minmax(0,1fr)]"
              : "grid-cols-[220px_minmax(0,1fr)_0] max-lg:grid-cols-[210px_minmax(0,1fr)] max-md:grid-cols-[52px_minmax(0,1fr)]"
            : detailsOpen()
              ? "grid-cols-[0_minmax(0,1fr)_326px] max-lg:grid-cols-[0_minmax(0,1fr)]"
              : "grid-cols-[0_minmax(0,1fr)_0] max-lg:grid-cols-[0_minmax(0,1fr)]",
        )}
      >
        <RefsSidebar repositoryId={repositoryId()} open={sidebarOpen()} />

        <HistoryTable
          repositoryId={repositoryId()}
          search={search()}
          selectedId={selectedId()}
          onSelect={(id) => setSelection({ repositoryId: repositoryId(), id })}
        />

        <aside
          class={cn(
            "min-h-0 min-w-0 overflow-hidden border-l border-border bg-panel transition-[opacity,transform] duration-150 motion-reduce:transition-none max-lg:absolute max-lg:top-0 max-lg:right-0 max-lg:bottom-0 max-lg:z-[5] max-lg:w-[340px] max-lg:shadow-[-18px_0_40px_rgba(5,3,7,.25)] max-sm:w-[min(340px,calc(100%_-_52px))]",
            !detailsOpen() && "pointer-events-none opacity-0 max-lg:translate-x-full",
          )}
        >
          <CommitDetails repositoryId={repositoryId()} selectedId={selectedId()} />
        </aside>
      </div>
    </div>
  );
}
