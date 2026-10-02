import { cn } from "cn";
import LoaderCircle from "lucide-solid/icons/loader-circle";
import Search from "lucide-solid/icons/search";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import { createEffect, createMemo, createSignal, Match, Show, Suspense, Switch } from "solid-js";

import { EmptyState } from "@/components/ui/empty-state";
import { UpdatingIndicator } from "@/components/ui/updating-indicator";
import { VirtualRows } from "@/components/ui/virtual-list";
import { useHistory } from "@/git/queries/history";
import { matchesSearch, searchNeedle } from "@/git/search";
import { useDelayed } from "@/hooks/delayed";

import { COLUMNS, MIN_WIDTH } from "./columns";
import { HistoryCommitRow } from "./commit-row";
import { graphWidth, ROW_HEIGHT } from "./history-graph";
import { HistoryWipRow } from "./wip-row";

export function HistoryTable(props: {
  repositoryId: string;
  search: string;
  selectedId: string | undefined;
  onSelect: (id: string) => void;
}) {
  const [scrollElement, setScrollElement] = createSignal<HTMLElement>();
  return (
    <main
      ref={setScrollElement}
      class="min-h-0 min-w-0 overflow-auto bg-bg"
      style={{ "--graph-width": `${graphWidth(1)}px` }}
    >
      <Suspense
        fallback={
          <>
            <HistoryHeader />
            <EmptyState icon={LoaderCircle} loading title="Loading history…" />
          </>
        }
      >
        <HistoryRows {...props} scrollElement={scrollElement()} />
      </Suspense>
    </main>
  );
}

/** The column headings; `updating` says the history is being reloaded. */
function HistoryHeader(props: { updating?: boolean }) {
  return (
    <div
      class={cn(
        "sticky top-0 z-10 grid h-[31px] items-center",
        COLUMNS,
        "border-b border-border bg-bg-soft text-[10.5px] font-[720] tracking-[.055em] text-faint uppercase [&>span]:flex [&>span]:h-full [&>span]:items-center [&>span]:border-r [&>span]:border-border-soft [&>span]:px-[9px]",
      )}
    >
      <span>Branch / tag</span>
      <span>Graph</span>
      <span class="justify-between gap-2">
        Commit message
        <Show when={props.updating}>
          <UpdatingIndicator />
        </Show>
      </span>
      <span>Date</span>
    </div>
  );
}

/**
 * The header and the rows. The header is rendered here, not in `HistoryTable`, so both are sized
 * to the graph's lanes as soon as the history is loaded.
 */
function HistoryRows(props: {
  repositoryId: string;
  search: string;
  selectedId: string | undefined;
  onSelect: (id: string) => void;
  /** The table's scroll container; only the rows in view are rendered. */
  scrollElement: HTMLElement | undefined;
}) {
  const history = useHistory(
    () => props.repositoryId,
    () => props.selectedId,
  );
  // Nothing selected yet, or the selected row is gone (e.g. the uncommitted changes, once
  // committed): report the row the table falls back to, so the details show it too.
  createEffect(() => {
    const row = history.selected();
    if (row && row.repositoryId === props.repositoryId && row.id !== props.selectedId) {
      props.onSelect(row.id);
    }
  });
  // The log is reloaded whenever a branch or tag changes, which takes a while in a big repository.
  const updating = useDelayed(() => history.log.isRefetching);
  const lanes = createMemo(() =>
    Math.max(1, ...history.rows().map((row) => row.graph?.width ?? 1)),
  );
  const needle = createMemo(() => searchNeedle(props.search));
  const visibleRows = createMemo(() =>
    needle() ? history.rows().filter((row) => matchesSearch(row, needle())) : history.rows(),
  );

  return (
    <div class={MIN_WIDTH} style={{ "--graph-width": `${graphWidth(lanes())}px` }}>
      <HistoryHeader updating={updating()} />
      <div role="listbox" aria-label="Commit history">
        <Show
          when={!history.log.error}
          fallback={
            <EmptyState icon={TriangleAlert} title="Couldn't load history" tone="error">
              {history.log.error?.message}
            </EmptyState>
          }
        >
          <Show
            when={visibleRows().length}
            fallback={
              <EmptyState icon={Search} title="No commits found">
                Try a different message, author, or SHA.
              </EmptyState>
            }
          >
            {/* By position rather than object identity: the rows are rebuilt whenever the log or
            the uncommitted changes change, and re-creating them would restart their queries. */}
            <VirtualRows
              items={visibleRows()}
              rowHeight={ROW_HEIGHT}
              scrollElement={props.scrollElement}
            >
              {(row, index) => {
                const asWip = () => {
                  const current = row();
                  return current.kind === "wip" ? current : undefined;
                };
                const asCommit = () => {
                  const current = row();
                  return current.kind === "commit" ? current : undefined;
                };
                const rowProps = {
                  get selected() {
                    return history.selected()?.id === row().id;
                  },
                  get position() {
                    return { index, count: visibleRows().length };
                  },
                  onSelect: () => props.onSelect(row().id),
                };
                return (
                  <Switch>
                    <Match when={asWip()}>
                      {(wip) => <HistoryWipRow {...rowProps} row={wip()} searching={!!needle()} />}
                    </Match>
                    <Match when={asCommit()}>
                      {(commit) => (
                        <HistoryCommitRow {...rowProps} commit={commit()} search={needle()} />
                      )}
                    </Match>
                  </Switch>
                );
              }}
            </VirtualRows>
          </Show>
        </Show>
      </div>
    </div>
  );
}
