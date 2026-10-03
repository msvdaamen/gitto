import { createFileRoute } from "@tanstack/solid-router";
import { cn } from "cn";
import { createMemo, createSignal, Show } from "solid-js";

import { CommitDetails } from "@/components/repository/details/commit-details";
import { HistoryTable } from "@/components/repository/history/history-table";
import { RefsSidebar } from "@/components/repository/sidebar/refs-sidebar";
import { RepositoryToolbar } from "@/components/repository/toolbar";
import { ResizeHandle } from "@/components/ui/resize-handle";
import { useRepositoryWatcher } from "@/git/queries/watch";
import { usePanelWidth } from "@/hooks/panel-width";
import { useSettled } from "@/hooks/settled";

// The panels can't take more than a share of the window either (see the grid below), so the history
// keeps room on small screens.
const SIDEBAR_BOUNDS = { initial: 220, min: 160, max: 480 };
const DETAILS_BOUNDS = { initial: 326, min: 280, max: 640 };

export const Route = createFileRoute("/(main)/$repoId")({
  component: RouteComponent,
});

function RouteComponent() {
  // Not thrown on once the route is left: effects on this page can still run until it's removed.
  const params = Route.useParams({ shouldThrow: false });
  // The repository in the URL; the last one once the route is left.
  const repositoryId = createMemo((last: string) => params()?.repoId ?? last, "");
  useRepositoryWatcher(repositoryId);

  const [search, setSearch] = createSignal("");
  // Tagged with its repository: the route component is reused when switching repositories, and the
  // new one starts without a selection.
  const [selection, setSelection] = createSignal<{ repositoryId: string; id: string }>();
  const selectedId = () => {
    const current = selection();
    return current?.repositoryId === repositoryId() ? current.id : undefined;
  };
  // What the details show: the selected row, but only the first and the last while the selection
  // moves quickly (an arrow key held down in the history), rather than loading every row passed.
  const detailsId = useSettled(selectedId);
  const [sidebarOpen, setSidebarOpen] = createSignal(true);
  const [detailsOpen, setDetailsOpen] = createSignal(true);
  const sidebar = usePanelWidth("sidebar", SIDEBAR_BOUNDS);
  const details = usePanelWidth("details", DETAILS_BOUNDS);

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
          "relative grid min-h-0 min-w-0 grid-cols-[var(--sidebar-width)_minmax(0,1fr)_var(--details-width)] overflow-hidden",
          // Below `md` the open sidebar is a rail of icons.
          sidebarOpen()
            ? "[--sidebar-width:min(var(--sidebar-size),30vw)] max-md:[--sidebar-width:52px]"
            : "[--sidebar-width:0px]",
          // Below `lg` the details slide over the history instead of taking a column.
          detailsOpen()
            ? "[--details-width:min(var(--details-size),40vw)] max-lg:[--details-width:0px]"
            : "[--details-width:0px]",
        )}
        style={{
          "--sidebar-size": `${sidebar.width()}px`,
          "--details-size": `${details.width()}px`,
        }}
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
          <CommitDetails repositoryId={repositoryId()} selectedId={detailsId()} />
        </aside>

        <Show when={sidebarOpen()}>
          <ResizeHandle
            edge="start"
            label="Resize sidebar"
            width={sidebar.width()}
            bounds={sidebar.bounds}
            onResize={sidebar.setWidth}
            onReset={sidebar.reset}
            class="left-(--sidebar-width) max-md:hidden"
          />
        </Show>
        <Show when={detailsOpen()}>
          <ResizeHandle
            edge="end"
            label="Resize details"
            width={details.width()}
            bounds={details.bounds}
            onResize={details.setWidth}
            onReset={details.reset}
            class="right-(--details-width) max-lg:hidden"
          />
        </Show>
      </div>
    </div>
  );
}
