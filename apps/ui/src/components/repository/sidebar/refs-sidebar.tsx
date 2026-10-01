import type { Ref } from "@gitto/git/types";
import { Popover } from "@kobalte/core/popover";
import { cn } from "cn";
import type { LucideIcon } from "lucide-solid";
import Archive from "lucide-solid/icons/archive";
import Bot from "lucide-solid/icons/bot";
import ChevronDown from "lucide-solid/icons/chevron-down";
import ChevronRight from "lucide-solid/icons/chevron-right";
import Cloud from "lucide-solid/icons/cloud";
import File from "lucide-solid/icons/file";
import Folder from "lucide-solid/icons/folder";
import FolderOpen from "lucide-solid/icons/folder-open";
import GitBranch from "lucide-solid/icons/git-branch";
import GitMerge from "lucide-solid/icons/git-merge";
import Inbox from "lucide-solid/icons/inbox";
import ListTree from "lucide-solid/icons/list-tree";
import LoaderCircle from "lucide-solid/icons/loader-circle";
import Settings from "lucide-solid/icons/settings";
import Tag from "lucide-solid/icons/tag";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import { createMemo, createSignal, For, Show, Suspense } from "solid-js";
import type { JSX } from "solid-js";

import { EmptyState } from "@/components/ui/empty-state";
import { Mascot } from "@/components/ui/mascot";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { toneClasses } from "@/components/ui/tone";
import { VirtualList } from "@/components/ui/virtual-list";
import { useRefs } from "@/git/queries/refs";
import { useStatus } from "@/git/queries/status";
import { buildRefTree, flattenRefTree } from "@/git/ref-tree";
import type { RefTreeRow } from "@/git/ref-tree";
import { useCollapsed } from "@/hooks/collapsed";

const MODES = [
  { label: "List", icon: ListTree },
  { label: "Agents", icon: Bot },
];

export function RefsSidebar(props: { repositoryId: string; open: boolean }) {
  const [mode, setMode] = createSignal("List");

  return (
    <aside
      class={cn(
        "relative flex min-h-0 min-w-0 flex-col overflow-hidden border-r border-border bg-panel transition-opacity duration-150 motion-reduce:transition-none max-md:w-[52px]",
        !props.open && "pointer-events-none opacity-0",
      )}
    >
      <div class="px-2.5 pt-[9px] pb-2 max-md:hidden">
        <SegmentedControl
          value={mode()}
          options={MODES.map((option) => option.label)}
          onChange={setMode}
        />
      </div>
      <RailModeSwitch value={mode()} onChange={setMode} />
      <Show when={mode() === "List"} fallback={<AgentPlaceholder />}>
        <nav
          class="flex min-h-0 flex-1 flex-col overflow-hidden pb-[35px] max-md:items-center max-md:gap-1 max-md:pt-2"
          aria-label="Repository references"
        >
          <Suspense
            fallback={
              <EmptyState icon={LoaderCircle} loading title="Loading branches…" class="h-[150px]" />
            }
          >
            <RefList repositoryId={props.repositoryId} />
          </Suspense>
        </nav>
      </Show>
      <button class="absolute right-0 bottom-0 left-0 flex h-[34px] cursor-pointer items-center gap-[7px] border-0 border-t border-border-soft bg-panel px-3 text-[9px] text-faint hover:text-text-soft max-md:justify-center max-md:px-0 max-md:[&>span]:hidden">
        <Settings size={15} />
        <span>Configure sidebar</span>
      </button>
    </aside>
  );
}

/** The List / Agents switch as a column of icons, for the narrow sidebar below 900px. */
function RailModeSwitch(props: { value: string; onChange: (value: string) => void }) {
  return (
    <div
      class="hidden flex-col items-center gap-1 border-b border-border-soft py-2 max-md:flex"
      role="group"
      aria-label="Sidebar view"
    >
      <For each={MODES}>
        {(option) => (
          <button
            class={cn(
              "grid size-9 cursor-pointer place-items-center rounded-lg border-0 bg-transparent text-muted hover:bg-panel-hover hover:text-text focus-ring-inset",
              props.value === option.label &&
                "bg-primary-soft text-primary-strong hover:bg-primary-soft hover:text-primary-strong",
            )}
            title={option.label}
            aria-label={option.label}
            aria-pressed={props.value === option.label}
            onClick={() => props.onChange(option.label)}
          >
            <option.icon size={16} />
          </button>
        )}
      </For>
    </div>
  );
}

/**
 * Height of an expanded section without its rows: its top border, its header (`h-[30px]`), the
 * border below that and the list's padding.
 */
const SECTION_CHROME = 1 + 30 + 1 + 2 * 4;
/** Space above a section's first row and below its last one. */
const LIST_PADDING = 4;
/** Height of every row in a section: 28px (`h-7`) plus the 1px gap below it. */
const ROW_HEIGHT = 29;
/** Indentation per tree level, in pixels. */
const DEPTH_INDENT = 12;

const PULL_REQUESTS = [
  { label: "#24 Polish desktop shell", meta: "open" },
  { label: "#18 Theme tokens", meta: "merged" },
];
const STASHES = [{ label: "WIP: layout experiment" }];

function RefList(props: { repositoryId: string }) {
  const status = useStatus(() => props.repositoryId);
  const refs = useRefs(() => props.repositoryId);

  const ofKind = (kind: Ref["kind"]) => (refs.data ?? []).filter((ref) => ref.kind === kind);
  const localBranches = createMemo(() => ofKind("local"));
  const remoteBranches = createMemo(() => ofKind("remote"));
  const tags = createMemo(() => ofKind("tag"));

  // Sections and ref folders share one saved state; folder ids are full ref paths like
  // `refs/heads/feature`, so they can't clash with the section ids.
  const collapsed = useCollapsed(() => props.repositoryId, { tags: true, stashes: true });
  const localRows = createMemo(() =>
    flattenRefTree(buildRefTree(localBranches()), collapsed.isCollapsed),
  );
  const remoteRows = createMemo(() =>
    flattenRefTree(buildRefTree(remoteBranches()), collapsed.isCollapsed),
  );

  /** A ref tree's line: a collapsible folder, or a ref drawn by `ref`. */
  const treeRow = (
    { node, depth }: RefTreeRow,
    ref: (ref: Ref, label: string, depth: number) => JSX.Element,
  ) =>
    node.type === "folder" ? (
      <SidebarFolder
        name={node.name}
        count={node.count}
        depth={depth}
        collapsed={collapsed.isCollapsed(node.path)}
        onToggle={() => collapsed.toggle(node.path)}
      />
    ) : (
      ref(node.ref, node.name, depth)
    );

  /** The props a section needs to collapse, saved under `id`. */
  const collapsible = (id: string) => ({
    get collapsed() {
      return collapsed.isCollapsed(id);
    },
    onToggle: () => collapsed.toggle(id),
  });

  return (
    <>
      <Show when={refs.error}>
        {(error) => (
          <EmptyState
            icon={TriangleAlert}
            title="Couldn't load branches"
            tone="error"
            class="h-[150px] shrink-0"
          >
            {error().message}
          </EmptyState>
        )}
      </Show>
      <SidebarSection
        title="Workspace"
        icon={Folder}
        count={1}
        {...collapsible("workspace")}
        items={["working-directory"]}
      >
        {() => (
          <SidebarRow
            icon={File}
            label="Working directory"
            active
            count={status.data?.files.length ?? 0}
            tone="amber"
          />
        )}
      </SidebarSection>
      <SidebarSection
        title="Local branches"
        icon={GitBranch}
        count={localBranches().length}
        {...collapsible("local")}
        items={localRows()}
      >
        {(row) =>
          treeRow(row, (branch, label, depth) => (
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
          ))
        }
      </SidebarSection>
      <SidebarSection
        title="Remotes"
        icon={Cloud}
        count={remoteBranches().length}
        {...collapsible("remotes")}
        items={remoteRows()}
      >
        {(row) =>
          treeRow(row, (branch, label, depth) => (
            <SidebarRow icon={GitBranch} label={label} title={branch.name} depth={depth} />
          ))
        }
      </SidebarSection>
      <SidebarSection
        title="Pull requests"
        icon={GitMerge}
        count={PULL_REQUESTS.length}
        {...collapsible("pullRequests")}
        items={PULL_REQUESTS}
      >
        {(pr) => <SidebarRow icon={GitMerge} label={pr.label} meta={pr.meta} />}
      </SidebarSection>
      <SidebarSection
        title="Tags"
        icon={Tag}
        count={tags().length}
        {...collapsible("tags")}
        items={tags()}
      >
        {(tag) => <SidebarRow icon={Tag} label={tag.name} />}
      </SidebarSection>
      <SidebarSection
        title="Stashes"
        icon={Inbox}
        count={STASHES.length}
        {...collapsible("stashes")}
        items={STASHES}
      >
        {(stash) => <SidebarRow icon={Archive} label={stash.label} />}
      </SidebarSection>
    </>
  );
}

/**
 * A section of the sidebar, like GitKraken's: its header always shows, and while expanded its rows
 * scroll on their own, only rendering the ones in view. Expanded sections share the sidebar's
 * height, but never take more than their rows need. Below 900px the section is an icon in a rail
 * instead, that opens its rows in a popover.
 */
function SidebarSection<T>(props: {
  title: string;
  icon: LucideIcon;
  count: number;
  collapsed: boolean;
  onToggle: () => void;
  items: T[];
  children: (item: T) => JSX.Element;
}) {
  const expanded = () => !props.collapsed && props.items.length > 0;

  return (
    <section
      class={cn(
        "flex min-h-[31px] flex-col border-t border-border-soft max-md:min-h-0 max-md:flex-none max-md:border-0",
        expanded() ? "flex-1" : "flex-none",
      )}
      style={{
        "max-height": expanded()
          ? `${SECTION_CHROME + props.items.length * ROW_HEIGHT}px`
          : undefined,
      }}
    >
      <button
        class={cn(
          "flex h-[30px] w-full shrink-0 cursor-pointer items-center gap-1.5 border-0 bg-panel-raised px-2.5 text-left text-muted hover:bg-panel-hover hover:text-text-soft focus-ring-inset max-md:hidden",
          !props.collapsed && "text-text-soft",
        )}
        aria-expanded={!props.collapsed ? "true" : "false"}
        onClick={props.onToggle}
      >
        <ChevronRight
          size={12}
          class={cn(
            "shrink-0 text-faint transition-transform duration-150 motion-reduce:transition-none",
            !props.collapsed && "rotate-90",
          )}
        />
        <props.icon size={13} class="shrink-0" />
        <SectionTitle title={props.title} count={props.count} />
      </button>
      <Show when={expanded()}>
        <VirtualList
          items={props.items}
          rowHeight={ROW_HEIGHT}
          padding={LIST_PADDING}
          class="border-t border-border-soft px-1.5 max-md:hidden"
        >
          {props.children}
        </VirtualList>
      </Show>
      <RailSection title={props.title} icon={props.icon} count={props.count} items={props.items}>
        {props.children}
      </RailSection>
    </section>
  );
}

/** A section's title, with how many items it has. */
function SectionTitle(props: { title: string; count: number }) {
  return (
    <>
      <span class="min-w-0 flex-1 truncate text-[9px] font-[720] tracking-[.06em] uppercase">
        {props.title}
      </span>
      <span class="shrink-0 rounded-full bg-bg px-1.5 py-px text-[8px] font-[650] text-faint tabular-nums">
        {props.count}
      </span>
    </>
  );
}

/**
 * A section in the narrow sidebar: its icon, with a badge counting its items, that opens the
 * section's rows in a popover beside the rail.
 */
function RailSection<T>(props: {
  title: string;
  icon: LucideIcon;
  count: number;
  items: T[];
  children: (item: T) => JSX.Element;
}) {
  return (
    <Popover placement="right-start" gutter={10}>
      <Popover.Trigger
        class="relative hidden size-9 cursor-pointer place-items-center rounded-lg border-0 bg-transparent text-muted hover:bg-panel-hover hover:text-text focus-ring-inset data-expanded:bg-primary-soft data-expanded:text-primary-strong max-md:grid"
        title={props.title}
        aria-label={`${props.title} (${props.count})`}
      >
        <props.icon size={16} />
        <Show when={props.count > 0}>
          <span class="absolute -top-0.5 -right-1 min-w-[16px] rounded-full border-2 border-panel bg-panel-active px-[3px] text-center text-[7.5px] leading-[11px] font-[700] text-text-soft tabular-nums">
            {compactCount(props.count)}
          </span>
        </Show>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content class="z-50 flex w-[260px] flex-col overflow-hidden rounded-lg border border-border bg-panel shadow-app outline-none">
          <Popover.Title class="flex h-[30px] shrink-0 items-center gap-1.5 border-b border-border-soft bg-panel-raised px-2.5 text-text-soft">
            <props.icon size={13} class="shrink-0" />
            <SectionTitle title={props.title} count={props.count} />
          </Popover.Title>
          <Show
            when={props.items.length}
            fallback={<p class="px-3 py-4 text-center text-[10px] text-faint">Nothing here yet</p>}
          >
            <VirtualList
              items={props.items}
              rowHeight={ROW_HEIGHT}
              padding={LIST_PADDING}
              class="max-h-[min(400px,70vh)] px-1.5"
            >
              {props.children}
            </VirtualList>
          </Show>
        </Popover.Content>
      </Popover.Portal>
    </Popover>
  );
}

/** A count short enough for a badge: `1.2k` for 1234. */
function compactCount(count: number): string {
  if (count < 1000) return String(count);
  return `${(count / 1000).toFixed(count < 10_000 ? 1 : 0).replace(/\.0$/, "")}k`;
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
      class="grid h-7 w-full cursor-pointer grid-cols-[12px_16px_minmax(0,1fr)_auto] items-center gap-[5px] rounded-[5px] border-0 bg-transparent pr-[7px] text-left text-muted hover:bg-panel-hover hover:text-text-soft focus-ring-inset"
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
        "grid h-7 w-full cursor-pointer grid-cols-[16px_minmax(0,1fr)_auto_auto] items-center gap-[5px] rounded-[5px] border-0 bg-transparent pr-[7px] text-left text-muted hover:bg-panel-hover hover:text-text-soft focus-ring-inset",
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
    <div class="flex flex-col items-center px-[18px] py-[65px] text-center max-md:px-0 max-md:py-4 max-md:[&>:not(:first-child)]:hidden">
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
