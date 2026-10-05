import type { ChangedFile } from "@gitto/git/types";
import { useQueryClient } from "@tanstack/solid-query";
import { createFileRoute } from "@tanstack/solid-router";
import { cn } from "cn";
import { createEffect, createMemo, createSignal, Show, Suspense } from "solid-js";

import { CommitDetails } from "@/components/repository/details/commit-details";
import { FileDiffView, prefetchFileDiff } from "@/components/repository/diff/file-diff-view";
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

  const queryClient = useQueryClient();
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
  // Settled with its repository, as it lags behind: the details never get another repository's row.
  const settledSelection = useSettled(selection);
  const detailsId = () => {
    const current = settledSelection();
    return current?.repositoryId === repositoryId() ? current.id : undefined;
  };
  // The file whose changes are shown in place of the history, with the row it's from: closed once
  // the details show another row, or another repository, so going back to it doesn't open them
  // again. Not shown from then on, before it's closed.
  const [openFile, setOpenFile] = createSignal<{
    repositoryId: string;
    rowId: string;
    file: ChangedFile;
  }>();
  const shownFile = () => {
    const current = openFile();
    return current?.repositoryId === repositoryId() && current.rowId === detailsId()
      ? current
      : undefined;
  };
  createEffect(() => {
    if (openFile() && !shownFile()) setOpenFile(undefined);
  });
  const openFileOf = (rowId: string) => (file: ChangedFile) =>
    setOpenFile({ repositoryId: repositoryId(), rowId, file });
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

        {/* The history and a file's changes share the column; the history stays underneath, so
            it's still scrolled to where it was when the changes are closed. */}
        <div class="grid min-h-0 min-w-0 grid-rows-[minmax(0,1fr)]">
          <div
            // Isolated, so its sticky header stays under the changes.
            class="isolate col-start-1 row-start-1 grid min-h-0 min-w-0"
            inert={shownFile() ? true : undefined}
          >
            <HistoryTable
              repositoryId={repositoryId()}
              search={search()}
              selectedId={selectedId()}
              detailsId={detailsId()}
              onSelect={(id) => setSelection({ repositoryId: repositoryId(), id })}
            />
          </div>
          <Show when={shownFile()}>
            {(open) => (
              <div class="z-[1] col-start-1 row-start-1 min-h-0 min-w-0 bg-bg">
                {/* Its own boundary: one around the page would take the page off it while the
                    changes' queries start, even for a moment, and the history would lose its
                    scroll position with it. */}
                <Suspense>
                  <FileDiffView
                    repositoryId={open().repositoryId}
                    sha={open().rowId}
                    file={open().file}
                    onOpen={openFileOf(open().rowId)}
                    onClose={() => setOpenFile(undefined)}
                  />
                </Suspense>
              </div>
            )}
          </Show>
        </div>

        <aside
          class={cn(
            "min-h-0 min-w-0 overflow-hidden border-l border-border bg-panel transition-[opacity,transform] duration-150 motion-reduce:transition-none max-lg:absolute max-lg:top-0 max-lg:right-0 max-lg:bottom-0 max-lg:z-[5] max-lg:w-[340px] max-lg:shadow-[-18px_0_40px_rgba(5,3,7,.25)] max-sm:w-[min(340px,calc(100%_-_52px))]",
            !detailsOpen() && "pointer-events-none opacity-0 max-lg:translate-x-full",
          )}
        >
          <CommitDetails
            repositoryId={repositoryId()}
            selectedId={detailsId()}
            onOpenFile={(file) => {
              const rowId = detailsId();
              if (rowId) openFileOf(rowId)(file);
            }}
            openPath={shownFile()?.file.path}
            onPrefetchFile={(file) => {
              const rowId = detailsId();
              if (rowId) prefetchFileDiff(queryClient, repositoryId(), rowId, file);
            }}
          />
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
