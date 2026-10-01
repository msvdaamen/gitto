import type { Ref } from "@gitto/git/types";
import { createVirtualizer } from "@tanstack/solid-virtual";
import { cn } from "cn";
import type { LucideIcon } from "lucide-solid";
import Archive from "lucide-solid/icons/archive";
import ChevronDown from "lucide-solid/icons/chevron-down";
import ChevronRight from "lucide-solid/icons/chevron-right";
import Cloud from "lucide-solid/icons/cloud";
import File from "lucide-solid/icons/file";
import Folder from "lucide-solid/icons/folder";
import FolderOpen from "lucide-solid/icons/folder-open";
import GitBranch from "lucide-solid/icons/git-branch";
import GitMerge from "lucide-solid/icons/git-merge";
import Inbox from "lucide-solid/icons/inbox";
import LoaderCircle from "lucide-solid/icons/loader-circle";
import Settings from "lucide-solid/icons/settings";
import Tag from "lucide-solid/icons/tag";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import {
  createEffect,
  createMemo,
  createSignal,
  For,
  onCleanup,
  onMount,
  Show,
  Suspense,
} from "solid-js";
import type { JSX } from "solid-js";

import { EmptyState } from "@/components/ui/empty-state";
import { Mascot } from "@/components/ui/mascot";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { toneClasses } from "@/components/ui/tone";
import { buildRefTree, flattenRefTree } from "@/git/ref-tree";
import type { RefTreeNode } from "@/git/ref-tree";
import { useRefs } from "@/git/refs";
import { useStatus } from "@/git/status";
import { useCollapsed } from "@/hooks/collapsed";

export function RefsSidebar(props: { repositoryId: string; open: boolean }) {
  const [mode, setMode] = createSignal("List");
  let nav: HTMLElement | undefined;

  return (
    <aside
      class={cn(
        "relative flex min-h-0 min-w-0 flex-col overflow-hidden border-r border-border bg-panel transition-opacity duration-150 motion-reduce:transition-none max-[900px]:w-[52px]",
        !props.open && "pointer-events-none opacity-0",
      )}
    >
      <div class="pt-[9px] pr-2.5 pb-1.5 pl-2.5 max-[900px]:px-[7px] max-[900px]:py-2">
        <SegmentedControl value={mode()} options={["List", "Agents"]} onChange={setMode} />
      </div>
      <Show when={mode() === "List"} fallback={<AgentPlaceholder />}>
        <nav
          ref={(el) => (nav = el)}
          class="min-h-0 flex-1 overflow-y-auto pt-0.5 pr-[7px] pb-[35px] pl-[7px]"
          aria-label="Repository references"
        >
          <Suspense
            fallback={
              <EmptyState icon={LoaderCircle} title="Loading branches…" class="h-[150px]" />
            }
          >
            <RefList repositoryId={props.repositoryId} scrollElement={() => nav} />
          </Suspense>
        </nav>
      </Show>
      <button class="absolute right-0 bottom-0 left-0 flex h-[34px] cursor-pointer items-center gap-[7px] border-0 border-t border-border-soft bg-panel px-3 text-[9px] text-faint hover:text-text-soft max-[900px]:justify-center max-[900px]:px-0 max-[900px]:[&>span]:hidden">
        <Settings size={15} />
        <span>Configure sidebar</span>
      </button>
    </aside>
  );
}

/**
 * A line of the sidebar: a section's header or one of its rows. The sidebar is one flat list of
 * lines so it can render only the ones in view, as a repository can have thousands of branches.
 */
interface Line {
  /** Unique across the sidebar and stable across refetches, so its measured height is kept. */
  key: string;
  /** Its height in pixels, until it's rendered and measured. */
  size: number;
  /** Spacing below it. */
  class?: string;
  render: () => JSX.Element;
}

type Row = Pick<Line, "key" | "render">;

/** Height of a section header (`h-[27px]`). */
const HEADER_HEIGHT = 27;
/** Height of a ref or folder row (`h-7`). */
const ROW_HEIGHT = 28;
/** Space between a section's rows, and below a section. */
const ROW_GAP = 1;
const SECTION_GAP = 4;
/** Indentation per tree level, in pixels. */
const DEPTH_INDENT = 12;

function RefList(props: { repositoryId: string; scrollElement: () => HTMLElement | undefined }) {
  const status = useStatus(() => props.repositoryId);
  const refs = useRefs(() => props.repositoryId);

  const ofKind = (kind: Ref["kind"]) => (refs.data ?? []).filter((ref) => ref.kind === kind);
  const localBranches = createMemo(() => ofKind("local"));
  const remoteBranches = createMemo(() => ofKind("remote"));
  const localTree = createMemo(() => buildRefTree(localBranches()));
  const remoteTree = createMemo(() => buildRefTree(remoteBranches()));
  const tags = createMemo(() => ofKind("tag"));

  // Sections and ref folders share one saved state; folder ids are full ref paths like
  // `refs/heads/feature`, so they can't clash with the section ids.
  const collapsed = useCollapsed(() => props.repositoryId, { tags: true, stashes: true });

  /**
   * A section's header, followed by its rows unless it's collapsed. `rows` is only called for an
   * expanded section. Below 900px the sidebar only shows the headers.
   */
  function section(
    header: { id: string; title: string; icon: LucideIcon; count: number; locked?: boolean },
    rows: () => Row[],
  ): Line[] {
    const isCollapsed = !header.locked && collapsed.isCollapsed(header.id);
    const shown = isCollapsed ? [] : rows();
    const headerLine: Line = {
      key: `section:${header.id}`,
      size: HEADER_HEIGHT + (shown.length ? 0 : SECTION_GAP),
      class: shown.length ? "max-[900px]:pb-1" : "pb-1",
      render: () => (
        <SectionHeader
          title={header.title}
          icon={header.icon}
          count={header.count}
          collapsed={isCollapsed}
          locked={header.locked}
          onToggle={() => collapsed.toggle(header.id)}
        />
      ),
    };
    return [
      headerLine,
      ...shown.map((row, index): Line => {
        const last = index === shown.length - 1;
        return {
          key: row.key,
          render: row.render,
          size: ROW_HEIGHT + (last ? SECTION_GAP : ROW_GAP),
          class: cn(last ? "pb-1" : "pb-px", "max-[900px]:hidden"),
        };
      }),
    ];
  }

  /** Refs grouped into collapsible folders on `/`. Folders start expanded. */
  function treeRows(
    tree: RefTreeNode[],
    row: (ref: Ref, label: string, depth: number) => JSX.Element,
  ): Row[] {
    return flattenRefTree(tree, collapsed.isCollapsed).map(({ node, depth }) =>
      node.type === "folder"
        ? {
            key: `folder:${node.path}`,
            render: () => (
              <SidebarFolder
                name={node.name}
                count={node.count}
                depth={depth}
                collapsed={collapsed.isCollapsed(node.path)}
                onToggle={() => collapsed.toggle(node.path)}
              />
            ),
          }
        : { key: `ref:${node.ref.fullName}`, render: () => row(node.ref, node.name, depth) },
    );
  }

  const lines = createMemo((): Line[] => [
    ...(refs.error
      ? [
          {
            key: "error",
            size: 150,
            render: () => (
              <EmptyState
                icon={TriangleAlert}
                title="Couldn't load branches"
                tone="error"
                class="h-[150px]"
              >
                {refs.error?.message}
              </EmptyState>
            ),
          },
        ]
      : []),
    ...section(
      { id: "workspace", title: "Workspace", icon: Folder, count: 1, locked: true },
      () => [
        {
          key: "workspace",
          render: () => (
            <SidebarRow
              icon={File}
              label="Working directory"
              active
              count={status.data?.files.length ?? 0}
              tone="amber"
            />
          ),
        },
      ],
    ),
    ...section(
      {
        id: "local",
        title: "Local branches",
        icon: GitBranch,
        count: localBranches().length,
        locked: true,
      },
      () =>
        treeRows(localTree(), (branch, label, depth) => (
          <SidebarRow
            icon={GitBranch}
            label={label}
            title={branch.name}
            depth={depth}
            active={branch.current}
            meta={
              branch.ahead ? `↑${branch.ahead}` : branch.behind ? `↓${branch.behind}` : undefined
            }
          />
        )),
    ),
    ...section(
      { id: "remotes", title: "Remotes", icon: Cloud, count: remoteBranches().length },
      () =>
        treeRows(remoteTree(), (branch, label, depth) => (
          <SidebarRow icon={GitBranch} label={label} title={branch.name} depth={depth} />
        )),
    ),
    ...section({ id: "pullRequests", title: "Pull requests", icon: GitMerge, count: 2 }, () => [
      {
        key: "pr:24",
        render: () => <SidebarRow icon={GitMerge} label="#24 Polish desktop shell" meta="open" />,
      },
      {
        key: "pr:18",
        render: () => <SidebarRow icon={GitMerge} label="#18 Theme tokens" meta="merged" />,
      },
    ]),
    ...section({ id: "tags", title: "Tags", icon: Tag, count: tags().length }, () =>
      tags().map((tag) => ({
        key: `ref:${tag.fullName}`,
        render: () => <SidebarRow icon={Tag} label={tag.name} />,
      })),
    ),
    ...section({ id: "stashes", title: "Stashes", icon: Inbox, count: 1 }, () => [
      {
        key: "stash:0",
        render: () => <SidebarRow icon={Archive} label="WIP: layout experiment" />,
      },
    ]),
  ]);

  // The virtualizer takes the window to observe from the scroll element's document when it first
  // gets one, so only hand it over once it's on the page: on mount it can still be detached, being
  // rendered inside a `Suspense` boundary.
  const [scrollElement, setScrollElement] = createSignal<HTMLElement>();
  onMount(() => {
    let frame = 0;
    const attach = () => {
      const element = props.scrollElement();
      if (element?.isConnected) setScrollElement(element);
      else frame = requestAnimationFrame(attach);
    };
    attach();
    onCleanup(() => cancelAnimationFrame(frame));
  });

  const virtualizer = createVirtualizer({
    get count() {
      return lines().length;
    },
    getScrollElement: () => scrollElement() ?? null,
    estimateSize: (index) => lines()[index]?.size ?? ROW_HEIGHT,
    getItemKey: (index) => lines()[index]?.key ?? index,
    overscan: 10,
  });

  return (
    <div class="relative" style={{ height: `${virtualizer.getTotalSize()}px` }}>
      <For each={virtualizer.getVirtualItems()}>
        {(item) => {
          const line = () => lines()[item.index];
          let element: HTMLDivElement | undefined;
          // Measure once rendered (`data-index` is set by then), and again whenever another line
          // moves into this position, e.g. after a folder above it collapses.
          createEffect(() => {
            line();
            if (element) virtualizer.measureElement(element);
          });
          return (
            <div
              ref={(el) => (element = el)}
              data-index={item.index}
              class={cn("absolute inset-x-0 top-0", line()?.class)}
              style={{ transform: `translateY(${item.start}px)` }}
            >
              <Show when={line()} keyed>
                {(current) => current.render()}
              </Show>
            </div>
          );
        }}
      </For>
    </div>
  );
}

function SectionHeader(props: {
  title: string;
  icon: LucideIcon;
  count: number;
  collapsed: boolean;
  locked?: boolean;
  onToggle: () => void;
}) {
  return (
    <button
      class="grid h-[27px] w-full cursor-pointer grid-cols-[14px_16px_minmax(0,1fr)_auto] items-center gap-1 rounded-[5px] border-0 bg-transparent px-[5px] text-left text-muted hover:not-disabled:bg-panel-hover disabled:cursor-default max-[900px]:flex max-[900px]:h-[31px] max-[900px]:justify-center max-[900px]:p-0 max-[900px]:[&>svg:first-child]:hidden max-[900px]:[&>small]:hidden max-[900px]:[&>span]:hidden"
      aria-expanded={!props.collapsed ? "true" : "false"}
      onClick={props.onToggle}
      disabled={props.locked}
    >
      {props.collapsed ? <ChevronRight size={12} /> : <ChevronDown size={12} />}
      <props.icon size={14} />
      <span class="truncate text-[9px] font-[720] tracking-[.055em] uppercase">{props.title}</span>
      <small class="text-[8px] text-faint">{props.count}</small>
    </button>
  );
}

function SidebarFolder(props: {
  name: string;
  count: number;
  depth: number;
  collapsed: boolean;
  onToggle: () => void;
}) {
  return (
    <button
      class="grid h-7 w-full cursor-pointer grid-cols-[12px_16px_minmax(0,1fr)_auto] items-center gap-[5px] rounded-[5px] border-0 bg-transparent pr-[7px] text-left text-muted hover:bg-panel-hover hover:text-text-soft"
      style={{ "padding-left": `${8 + props.depth * DEPTH_INDENT}px` }}
      aria-expanded={!props.collapsed ? "true" : "false"}
      title={props.name}
      onClick={props.onToggle}
    >
      {props.collapsed ? <ChevronRight size={12} /> : <ChevronDown size={12} />}
      {props.collapsed ? <Folder size={13} /> : <FolderOpen size={13} />}
      <span class="truncate text-[10px]">{props.name}</span>
      <small class="text-[8px] text-faint">{props.count}</small>
    </button>
  );
}

function SidebarRow(props: {
  icon: LucideIcon;
  label: string;
  title?: string;
  depth?: number;
  active?: boolean;
  count?: number;
  meta?: string;
  tone?: "amber";
}) {
  return (
    <button
      class={cn(
        "grid h-7 w-full cursor-pointer grid-cols-[16px_minmax(0,1fr)_auto_auto] items-center gap-[5px] rounded-[5px] border-0 bg-transparent pr-[7px] text-left text-muted hover:bg-panel-hover hover:text-text-soft",
        props.active && "bg-primary-soft text-text [&>svg]:text-primary-strong",
      )}
      style={{ "padding-left": `${25 + (props.depth ?? 0) * DEPTH_INDENT}px` }}
      title={props.title ?? props.label}
    >
      <props.icon size={13} />
      <span class="truncate text-[10px]">{props.label}</span>
      {props.meta && <small class="text-[8px] text-blue">{props.meta}</small>}
      {props.count !== undefined && (
        <em
          class={cn(
            "min-w-[17px] rounded-lg px-1 py-0.5 text-center text-[8px] not-italic",
            toneClasses[props.tone ?? "neutral"],
          )}
        >
          {props.count}
        </em>
      )}
    </button>
  );
}

function AgentPlaceholder() {
  return (
    <div class="flex flex-col items-center px-[18px] py-[65px] text-center">
      <Mascot size={38} />
      <strong class="mt-3 text-[11px]">No agents running</strong>
      <p class="mt-1.5 mb-3 text-[9px] leading-1.5 text-muted">
        Agent sessions will appear here alongside their worktrees.
      </p>
      <button class="h-[27px] cursor-pointer rounded-md border border-border bg-primary-soft px-2.5 text-[9px] text-primary-strong">
        Start a session
      </button>
    </div>
  );
}
