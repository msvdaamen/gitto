import type { Ref } from "@gitto/git/types";
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
import { createMemo, createSignal, For, Show, Suspense } from "solid-js";
import type { JSX } from "solid-js";
import { Dynamic } from "solid-js/web";

import { EmptyState } from "@/components/ui/empty-state";
import { Mascot } from "@/components/ui/mascot";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { toneClasses } from "@/components/ui/tone";
import { buildRefTree } from "@/git/ref-tree";
import type { RefTreeNode } from "@/git/ref-tree";
import { useRefs } from "@/git/refs";
import { useStatus } from "@/git/status";
import { useCollapsed } from "@/hooks/collapsed";

export function RefsSidebar(props: { repositoryId: string; open: boolean }) {
  const [mode, setMode] = createSignal("List");

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
          class="min-h-0 flex-1 overflow-y-auto pt-0.5 pr-[7px] pb-[35px] pl-[7px]"
          aria-label="Repository references"
        >
          <Suspense
            fallback={
              <EmptyState icon={LoaderCircle} title="Loading branches…" class="h-[150px]" />
            }
          >
            <RefList repositoryId={props.repositoryId} />
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

function RefList(props: { repositoryId: string }) {
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

  return (
    <>
      <Show when={refs.error}>
        {(error) => (
          <EmptyState
            icon={TriangleAlert}
            title="Couldn't load branches"
            tone="error"
            class="h-[150px]"
          >
            {error().message}
          </EmptyState>
        )}
      </Show>
      <SidebarSection
        title="Workspace"
        icon={Folder}
        count={1}
        collapsed={false}
        onToggle={() => undefined}
        locked
      >
        <SidebarRow
          icon={File}
          label="Working directory"
          active
          count={status.data?.files.length ?? 0}
          tone="amber"
        />
      </SidebarSection>
      <SidebarSection
        title="Local branches"
        icon={GitBranch}
        count={localBranches().length}
        collapsed={false}
        onToggle={() => undefined}
        locked
      >
        <RefTree nodes={localTree()} collapsed={collapsed}>
          {(branch, label, depth) => (
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
          )}
        </RefTree>
      </SidebarSection>
      <SidebarSection
        title="Remotes"
        icon={Cloud}
        count={remoteBranches().length}
        collapsed={collapsed.isCollapsed("remotes")}
        onToggle={() => collapsed.toggle("remotes")}
      >
        <RefTree nodes={remoteTree()} collapsed={collapsed}>
          {(branch, label, depth) => (
            <SidebarRow icon={GitBranch} label={label} title={branch.name} depth={depth} />
          )}
        </RefTree>
      </SidebarSection>
      <SidebarSection
        title="Pull requests"
        icon={GitMerge}
        count={2}
        collapsed={collapsed.isCollapsed("pullRequests")}
        onToggle={() => collapsed.toggle("pullRequests")}
      >
        <SidebarRow icon={GitMerge} label="#24 Polish desktop shell" meta="open" />
        <SidebarRow icon={GitMerge} label="#18 Theme tokens" meta="merged" />
      </SidebarSection>
      <SidebarSection
        title="Tags"
        icon={Tag}
        count={tags().length}
        collapsed={collapsed.isCollapsed("tags")}
        onToggle={() => collapsed.toggle("tags")}
      >
        <For each={tags()}>{(tag) => <SidebarRow icon={Tag} label={tag.name} />}</For>
      </SidebarSection>
      <SidebarSection
        title="Stashes"
        icon={Inbox}
        count={1}
        collapsed={collapsed.isCollapsed("stashes")}
        onToggle={() => collapsed.toggle("stashes")}
      >
        <SidebarRow icon={Archive} label="WIP: layout experiment" />
      </SidebarSection>
    </>
  );
}

function SidebarSection(props: {
  title: string;
  icon: LucideIcon;
  count: number;
  collapsed: boolean;
  locked?: boolean;
  onToggle: () => void;
  children: JSX.Element;
}) {
  return (
    <section class="mb-1">
      <button
        class="grid h-[27px] w-full cursor-pointer grid-cols-[14px_16px_minmax(0,1fr)_auto] items-center gap-1 rounded-[5px] border-0 bg-transparent px-[5px] text-left text-muted hover:not-disabled:bg-panel-hover disabled:cursor-default max-[900px]:flex max-[900px]:h-[31px] max-[900px]:justify-center max-[900px]:p-0 max-[900px]:[&>svg:first-child]:hidden max-[900px]:[&>small]:hidden max-[900px]:[&>span]:hidden"
        aria-expanded={!props.collapsed ? "true" : "false"}
        onClick={props.onToggle}
        disabled={props.locked}
      >
        {props.collapsed ? <ChevronRight size={12} /> : <ChevronDown size={12} />}
        <Dynamic component={props.icon} size={14} />
        <span class="truncate text-[9px] font-[720] tracking-[.055em] uppercase">
          {props.title}
        </span>
        <small class="text-[8px] text-faint">{props.count}</small>
      </button>
      <Show when={!props.collapsed}>
        <div class="flex flex-col gap-px max-[900px]:hidden">{props.children}</div>
      </Show>
    </section>
  );
}

/** Refs grouped into collapsible folders on `/`. Folders start expanded. */
function RefTree(props: {
  nodes: RefTreeNode[];
  collapsed: ReturnType<typeof useCollapsed>;
  children: (ref: Ref, label: string, depth: number) => JSX.Element;
}) {
  const Nodes = (nodesProps: { nodes: RefTreeNode[]; depth: number }) => (
    <For each={nodesProps.nodes}>
      {(node) =>
        node.type === "folder" ? (
          <>
            <SidebarFolder
              name={node.name}
              count={node.count}
              depth={nodesProps.depth}
              collapsed={props.collapsed.isCollapsed(node.path)}
              onToggle={() => props.collapsed.toggle(node.path)}
            />
            <Show when={!props.collapsed.isCollapsed(node.path)}>
              <Nodes nodes={node.children} depth={nodesProps.depth + 1} />
            </Show>
          </>
        ) : (
          props.children(node.ref, node.name, nodesProps.depth)
        )
      }
    </For>
  );

  return <Nodes nodes={props.nodes} depth={0} />;
}

/** Indentation per tree level, in pixels. */
const DEPTH_INDENT = 12;

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
      <Dynamic component={props.icon} size={13} />
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
