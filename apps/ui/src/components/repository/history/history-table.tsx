import { cn } from "cn";
import LoaderCircle from "lucide-solid/icons/loader-circle";
import Search from "lucide-solid/icons/search";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import { createEffect, createMemo, createSignal, Match, Show, Suspense, Switch } from "solid-js";

import { EmptyState } from "@/components/ui/empty-state";
import { UpdatingIndicator } from "@/components/ui/updating-indicator";
import { VirtualRows } from "@/components/ui/virtual-list";
import { useHistory } from "@/git/queries/history";
import type { CommitRow, StashRow, WipRow } from "@/git/rows";
import { matchesSearch, searchNeedle } from "@/git/search";
import { useDelayed } from "@/hooks/delayed";
import type { ScrollId } from "@/lib/scroll";

import { AuthorTooltipProvider } from "./author-tooltip";
import { COLUMNS, MIN_WIDTH } from "./columns";
import { HistoryCommitRow } from "./commit-row";
import { graphWidth, ROW_HEIGHT } from "./history-graph";
import { optionId } from "./history-option";
import { HistoryStashRow } from "./stash-row";
import { HistoryWipRow } from "./wip-row";

export function HistoryTable(props: {
  repositoryId: string;
  search: string;
  selectedId: string | undefined;
  /** The row the details show: `selectedId`, once the selection stops moving quickly. */
  detailsId: string | undefined;
  onSelect: (id: string) => void;
}) {
  const [scrollElement, setScrollElement] = createSignal<HTMLElement>();
  return (
    <main
      ref={setScrollElement}
      data-scroll-restoration-id={"history" satisfies ScrollId}
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
        <AuthorTooltipProvider>
          <HistoryRows {...props} scrollElement={scrollElement()} />
        </AuthorTooltipProvider>
      </Suspense>
    </main>
  );
}

/** The column headings; `updating` says the history is being reloaded. */
function HistoryHeader(props: { updating?: boolean; ref?: (el: HTMLDivElement) => void }) {
  return (
    <div
      ref={props.ref}
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
  detailsId: string | undefined;
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

  let header: HTMLDivElement | undefined;
  let list: HTMLDivElement | undefined;
  // The selected row's place among the rows on show; -1 when the search hides it.
  const selectedIndex = createMemo(() => {
    const id = history.selected()?.id;
    return visibleRows().findIndex((row) => row.id === id);
  });

  /** Scrolls the row at `index` into view, clear of the header that sticks above the rows. */
  function reveal(index: number) {
    const scroller = props.scrollElement;
    if (!scroller || !list) return;
    const viewTop = scroller.getBoundingClientRect().top + scroller.clientTop;
    const top = list.getBoundingClientRect().top + index * ROW_HEIGHT;
    const above = viewTop + (header?.offsetHeight ?? 0) - top;
    const below = top + ROW_HEIGHT - (viewTop + scroller.clientHeight);
    if (above > 0) scroller.scrollTop -= above;
    else if (below > 0) scroller.scrollTop += below;
  }

  /** Moves the selection with the arrow keys, Page Up and Down, Home and End. */
  function onKeyDown(event: KeyboardEvent) {
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    const rows = visibleRows();
    const current = selectedIndex();
    // A page is what fits in view, less a row to keep one in sight.
    const view = (props.scrollElement?.clientHeight ?? 0) - (header?.offsetHeight ?? 0);
    const page = Math.max(1, Math.floor(view / ROW_HEIGHT) - 1);
    const targets: Record<string, number> = {
      ArrowDown: current + 1,
      ArrowUp: current - 1,
      PageDown: current + page,
      PageUp: current - page,
      Home: 0,
      End: rows.length - 1,
    };
    const target = targets[event.key];
    if (target === undefined || rows.length === 0) return;
    event.preventDefault();
    const index = Math.min(rows.length - 1, Math.max(0, target));
    if (index !== current) props.onSelect(rows[index]!.id);
    reveal(index);
  }

  return (
    <div class={MIN_WIDTH} style={{ "--graph-width": `${graphWidth(lanes())}px` }}>
      <HistoryHeader ref={(el) => (header = el)} updating={updating()} />
      <div
        ref={(el) => (list = el)}
        role="listbox"
        aria-label="Commit history"
        aria-activedescendant={selectedIndex() === -1 ? undefined : optionId(selectedIndex())}
        tabIndex={0}
        class="group/history outline-none"
        onKeyDown={onKeyDown}
        // A clicked row hands the focus to the list, which is what the keyboard moves through:
        // the rows come and go as the list scrolls, and would take the focus with them.
        onFocusIn={(event) => event.target !== list && list?.focus({ preventScroll: true })}
      >
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
                // The row as each kind; the last one of that kind while the row turns into
                // another, rather than a <Match>'s narrowed value. Switching repositories happens
                // in a transition, which removes what was rendered for the row only once it's
                // over, and its effects can still run in between: reading the narrowed value there
                // would throw.
                const wip = createMemo<WipRow | undefined>((last) => {
                  const current = row();
                  return current.kind === "wip" ? current : last;
                });
                const stash = createMemo<StashRow | undefined>((last) => {
                  const current = row();
                  return current.kind === "stash" ? current : last;
                });
                const commit = createMemo<CommitRow | undefined>((last) => {
                  const current = row();
                  return current.kind === "commit" ? current : last;
                });
                const rowProps = {
                  get selected() {
                    return history.selected()?.id === row().id;
                  },
                  get detailed() {
                    return props.detailsId === row().id;
                  },
                  get position() {
                    return { index, count: visibleRows().length };
                  },
                  onSelect: () => props.onSelect(row().id),
                };
                return (
                  <Switch>
                    <Match when={row().kind === "wip"}>
                      <HistoryWipRow {...rowProps} row={wip()!} searching={!!needle()} />
                    </Match>
                    <Match when={row().kind === "stash"}>
                      <HistoryStashRow {...rowProps} stash={stash()!} searching={!!needle()} />
                    </Match>
                    <Match when={row().kind === "commit"}>
                      <HistoryCommitRow {...rowProps} commit={commit()!} search={needle()} />
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
