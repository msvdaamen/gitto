import { cn } from "cn";
import Check from "lucide-solid/icons/check";
import Cloud from "lucide-solid/icons/cloud";
import Laptop from "lucide-solid/icons/laptop";
import LoaderCircle from "lucide-solid/icons/loader-circle";
import PencilLine from "lucide-solid/icons/pencil-line";
import Search from "lucide-solid/icons/search";
import Tag from "lucide-solid/icons/tag";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import { createEffect, createMemo, createSignal, Index, Show, Suspense, type JSX } from "solid-js";

import { Avatar } from "@/components/ui/avatar";
import { EmptyState } from "@/components/ui/empty-state";
import { LineStats } from "@/components/ui/line-stats";
import { toneClasses, type Tone } from "@/components/ui/tone";
import { useCommitFiles } from "@/git/diff";
import { useHistory } from "@/git/history";
import { toRefLabels, type RefLabel } from "@/git/ref-labels";
import { headLabel, useStatus } from "@/git/status";
import type { Commit, CommitRef } from "@/types/git";

import { graphWidth, HistoryGraph, laneColor } from "./history-graph";

/** The table's columns; the graph's is as wide as its lanes (see `graphWidth`). */
const COLUMNS =
  "min-w-[calc(560px+var(--graph-width))] grid-cols-[minmax(105px,.8fr)_var(--graph-width)_minmax(240px,2.2fr)_minmax(115px,.85fr)_100px]";

export function HistoryTable(props: {
  repositoryId: string;
  search: string;
  selectedId: string | undefined;
  onSelect: (id: string) => void;
}) {
  const [lanes, setLanes] = createSignal(1);

  return (
    <main
      class="min-h-0 min-w-0 overflow-auto bg-bg"
      style={{ "--graph-width": `${graphWidth(lanes())}px` }}
    >
      <div
        class={cn(
          "sticky top-0 z-10 grid h-[31px] items-center",
          COLUMNS,
          " border-b border-border bg-bg-soft text-[8px] font-[720] tracking-[.055em] text-faint uppercase [&>span]:flex [&>span]:h-full [&>span]:items-center [&>span]:border-r [&>span]:border-border-soft [&>span]:px-[9px]",
        )}
      >
        <span>Branch / tag</span>
        <span>Graph</span>
        <span>Commit message</span>
        <span>Author</span>
        <span>Date</span>
      </div>
      <div
        class="min-w-[calc(560px+var(--graph-width))]"
        role="listbox"
        aria-label="Commit history"
      >
        <Suspense fallback={<EmptyState icon={LoaderCircle} title="Loading history…" />}>
          <HistoryRows {...props} onLanes={setLanes} />
        </Suspense>
      </div>
    </main>
  );
}

function HistoryRows(props: {
  repositoryId: string;
  search: string;
  selectedId: string | undefined;
  onSelect: (id: string) => void;
  /** Reports how many lanes the graph needs, to size its column. */
  onLanes: (lanes: number) => void;
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
  createEffect(() =>
    props.onLanes(Math.max(1, ...history.rows().map((row) => row.graph?.width ?? 1))),
  );
  const visibleRows = createMemo(() => {
    const needle = props.search.trim().toLowerCase();
    return needle
      ? history
          .rows()
          .filter((commit) =>
            `${commit.message} ${commit.author} ${commit.id} ${commit.refs.map(refLabel).join(" ")}`
              .toLowerCase()
              .includes(needle),
          )
      : history.rows();
  });

  return (
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
        {/* By position rather than `<For>`'s object identity: the rows are rebuilt whenever the log
            or status is refetched, and re-creating them would restart their queries. */}
        <Index each={visibleRows()}>
          {(commit) => (
            <Show
              when={commit().isWip}
              fallback={
                <HistoryRow
                  commit={commit()}
                  selected={history.selected()?.id === commit().id}
                  onSelect={() => props.onSelect(commit().id)}
                />
              }
            >
              <WipRow
                commit={commit()}
                selected={history.selected()?.id === commit().id}
                onSelect={() => props.onSelect(commit().id)}
              />
            </Show>
          )}
        </Index>
      </Show>
    </Show>
  );
}

function HistoryRow(props: { commit: Commit; selected: boolean; onSelect: () => void }) {
  return (
    <button
      role="option"
      aria-selected={props.selected ? "true" : "false"}
      class={cn(
        COLUMNS,
        "group grid h-[47px] w-full cursor-pointer items-center border-0 border-b border-border-soft bg-transparent p-0 text-left text-muted hover:bg-panel-hover [&>span]:min-w-0 [&>span]:px-[9px]",
        props.selected &&
          "bg-[linear-gradient(90deg,var(--primary-soft),color-mix(in_srgb,var(--primary-soft)_35%,transparent))] text-text-soft shadow-[inset_2px_0_var(--primary)]",
      )}
      onClick={props.onSelect}
    >
      <RefLabels refs={props.commit.refs} color={laneColor(props.commit.graph?.column ?? 0)} />
      <div class="h-full overflow-x-clip">
        <Show when={props.commit.graph}>
          {(row) => (
            <HistoryGraph
              row={row()}
              initials={props.commit.initials}
              avatarColor={props.commit.avatarColor}
            />
          )}
        </Show>
      </div>
      <span class="flex min-w-0 items-center justify-between gap-[7px]">
        <strong class="truncate text-[10.5px] font-[570] text-text">{props.commit.message}</strong>
        <Show when={props.selected}>
          <Suspense>
            <CommitTotals commit={props.commit} />
          </Suspense>
        </Show>
      </span>
      <span class="flex items-center gap-[7px]">
        <Avatar initials={props.commit.initials} color={props.commit.avatarColor} />
        <span class="truncate text-[9.5px]">
          {props.commit.author.split(" ").map((part, index) => (
            <>
              {index > 0 && " "}
              <span>{part}</span>
            </>
          ))}
        </span>
      </span>
      <span class="truncate text-[9px]">{props.commit.timestamp}</span>
    </button>
  );
}

function refLabel(ref: CommitRef): string {
  return ref.kind === "tag" ? `tag: ${ref.name}` : ref.name;
}

/**
 * The commit's branches and tags, tinted in its lane's colour like GitKraken: the first one, with
 * a count of the rest. A branch shows whether it's local (laptop), on a remote (cloud) or both.
 */
function RefLabels(props: { refs: CommitRef[]; color: string }) {
  const labels = createMemo(() => toRefLabels(props.refs));

  return (
    <span class="flex items-center gap-1 overflow-hidden" style={{ "--lane": props.color }}>
      <Show when={labels()[0]}>{(label) => <RefPill label={label()} />}</Show>
      <Show when={labels().length > 1}>
        <span
          class="shrink-0 rounded-sm border border-[color-mix(in_srgb,var(--lane)_45%,var(--border))] px-[4px] py-[3px] text-[8px] font-[680] text-text-soft"
          title={labels()
            .slice(1)
            .map((label) => label.name)
            .join("\n")}
        >
          +{labels().length - 1}
        </span>
      </Show>
    </span>
  );
}

function RefPill(props: { label: RefLabel }) {
  const branch = () => (props.label.kind === "branch" ? props.label : undefined);

  return (
    <span
      class={cn(
        "flex min-w-0 items-center gap-[3px] rounded-sm border border-[color-mix(in_srgb,var(--lane)_45%,var(--border))] bg-[color-mix(in_srgb,var(--lane)_16%,transparent)] px-[5px] py-[3px] text-[8px] text-text-soft [&>svg]:shrink-0",
        branch()?.current && "font-[720] text-text",
      )}
      title={props.label.name}
    >
      <Show when={branch()?.current}>
        <Check size={9} strokeWidth={3} />
      </Show>
      <Show when={props.label.kind === "tag"}>
        <Tag size={9} strokeWidth={2.4} />
      </Show>
      <span class="truncate">{props.label.name}</span>
      <Show when={branch()?.local}>
        <Laptop size={9} strokeWidth={2.4} aria-label="Local" />
      </Show>
      <Show when={branch()?.remotes.length}>
        <Cloud size={9} strokeWidth={2.4} aria-label={`On ${branch()!.remotes.join(", ")}`} />
      </Show>
    </span>
  );
}

/**
 * The uncommitted changes, set apart from the commits below: an amber, dashed-off row with a
 * hollow graph node that counts what's staged and what isn't.
 */
function WipRow(props: { commit: Commit; selected: boolean; onSelect: () => void }) {
  const status = useStatus(() => props.commit.repositoryId);
  const counts = createMemo(() => {
    const files = status.data?.files ?? [];
    return {
      staged: files.filter((file) => file.staged && file.staged !== "conflicted").length,
      unstaged: files.filter((file) => file.unstaged && file.unstaged !== "conflicted").length,
      conflicted: files.filter((file) => file.staged === "conflicted").length,
    };
  });

  return (
    <button
      role="option"
      aria-selected={props.selected ? "true" : "false"}
      class={cn(
        COLUMNS,
        "grid h-[47px] w-full cursor-pointer items-center border-0 border-b border-dashed border-[color-mix(in_srgb,var(--amber)_45%,var(--border))] p-0 text-left text-muted shadow-[inset_2px_0_var(--amber)] [&>span]:min-w-0 [&>span]:px-[9px]",
        props.selected
          ? "bg-[linear-gradient(90deg,color-mix(in_srgb,var(--amber)_24%,transparent),color-mix(in_srgb,var(--amber)_8%,transparent))]"
          : "bg-[color-mix(in_srgb,var(--amber-soft)_70%,transparent)] hover:bg-amber-soft",
      )}
      onClick={props.onSelect}
    >
      <span class="flex">
        <span
          class={cn(
            "inline-flex items-center gap-1 rounded-sm border border-[color-mix(in_srgb,var(--amber)_40%,var(--border))] px-[5px] py-[3px] text-[8px] font-[680]",
            toneClasses.amber,
          )}
        >
          <PencilLine size={9} strokeWidth={2.4} />
          WIP
        </span>
      </span>
      <div class="h-full overflow-x-clip">
        <Show when={props.commit.graph}>{(row) => <HistoryGraph row={row()} wip />}</Show>
      </div>
      <span class="col-span-3 flex min-w-0 items-center gap-2">
        <strong class="truncate text-[10.5px] font-[620] text-text italic">
          {props.commit.message}
        </strong>
        <Show when={counts().staged}>
          <WipCount tone="mint">{counts().staged} staged</WipCount>
        </Show>
        <Show when={counts().unstaged}>
          <WipCount tone="amber">{counts().unstaged} unstaged</WipCount>
        </Show>
        <Show when={counts().conflicted}>
          <WipCount tone="coral">{counts().conflicted} conflicted</WipCount>
        </Show>
        <span class="ml-auto shrink-0 text-[9px] text-faint">
          on {status.data && headLabel(status.data.head)}
        </span>
      </span>
    </button>
  );
}

function WipCount(props: { tone: Tone; children: JSX.Element }) {
  return (
    <small
      class={cn(
        "shrink-0 rounded-full px-1.5 py-0.5 text-[8px] font-[680]",
        toneClasses[props.tone],
      )}
    >
      {props.children}
    </small>
  );
}

/** Line totals are only known for the selected commit, whose files are loaded anyway. */
function CommitTotals(props: { commit: Commit }) {
  const { totals } = useCommitFiles(
    () => props.commit.repositoryId,
    () => props.commit.id,
  );
  return (
    <LineStats
      additions={totals().additions}
      deletions={totals().deletions}
      class="shrink-0 text-[8px]"
    />
  );
}
