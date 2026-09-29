import { createFileRoute } from "@tanstack/solid-router";
import type { LucideIcon } from "lucide-solid";
import Archive from "lucide-solid/icons/archive";
import ArchiveRestore from "lucide-solid/icons/archive-restore";
import ChevronDown from "lucide-solid/icons/chevron-down";
import ChevronRight from "lucide-solid/icons/chevron-right";
import Cloud from "lucide-solid/icons/cloud";
import Copy from "lucide-solid/icons/copy";
import Download from "lucide-solid/icons/download";
import Ellipsis from "lucide-solid/icons/ellipsis";
import File from "lucide-solid/icons/file";
import Folder from "lucide-solid/icons/folder";
import GitBranch from "lucide-solid/icons/git-branch";
import GitCommitHorizontal from "lucide-solid/icons/git-commit-horizontal";
import GitMerge from "lucide-solid/icons/git-merge";
import Inbox from "lucide-solid/icons/inbox";
import PanelLeft from "lucide-solid/icons/panel-left";
import PanelRight from "lucide-solid/icons/panel-right";
import Redo2 from "lucide-solid/icons/redo-2";
import RefreshCw from "lucide-solid/icons/refresh-cw";
import Search from "lucide-solid/icons/search";
import Settings from "lucide-solid/icons/settings";
import Tag from "lucide-solid/icons/tag";
import Undo2 from "lucide-solid/icons/undo-2";
import Upload from "lucide-solid/icons/upload";
import { createMemo, createSignal, For, Show } from "solid-js";
import type { JSX } from "solid-js";
import { Dynamic } from "solid-js/web";

import { Avatar } from "@/components/ui/avatar";
import { Badge } from "@/components/ui/badge";
import { IconButton } from "@/components/ui/button";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { TextInput } from "@/components/ui/text-input";
import { commits, localBranches, remoteBranches, repositories } from "@/data/mock-data";
import type { Commit, FileStatus } from "@/types/git";

export const Route = createFileRoute("/(main)/$repoId")({
  component: RouteComponent,
});

const toolbarActions: { icon: LucideIcon; label: string; accent?: boolean }[] = [
  { icon: Undo2, label: "Undo" },
  { icon: Redo2, label: "Redo" },
  { icon: Download, label: "Pull", accent: true },
  { icon: Upload, label: "Push" },
  { icon: GitBranch, label: "Branch" },
  { icon: Archive, label: "Stash" },
  { icon: ArchiveRestore, label: "Pop" },
  { icon: RefreshCw, label: "Fetch" },
];

const fileStatusLabel: Record<FileStatus, string> = {
  modified: "M",
  added: "A",
  deleted: "D",
  renamed: "R",
};

function RouteComponent() {
  const params = Route.useParams();

  const repository = createMemo(
    () => repositories.find((repo) => repo.id === params().repoId) ?? repositories[0]!,
  );
  const [search, setSearch] = createSignal("");
  const [selectedId, setSelectedId] = createSignal("c1");
  const [sidebarMode, setSidebarMode] = createSignal("List");
  const [sidebarOpen, setSidebarOpen] = createSignal(true);
  const [detailsOpen, setDetailsOpen] = createSignal(true);
  const [collapsed, setCollapsed] = createSignal({
    remotes: false,
    pullRequests: false,
    tags: true,
    stashes: true,
  });
  const visibleCommits = createMemo(() => {
    const needle = search().trim().toLowerCase();
    return needle
      ? commits.filter((commit) =>
          `${commit.message} ${commit.author} ${commit.sha} ${commit.refs.join(" ")}`
            .toLowerCase()
            .includes(needle),
        )
      : commits;
  });
  const selectedCommit = createMemo(
    () => commits.find((commit) => commit.id === selectedId()) ?? commits[1]!,
  );
  const toggleSection = (id: keyof ReturnType<typeof collapsed>) =>
    setCollapsed((current) => ({ ...current, [id]: !current[id] }));

  return (
    <div class="grid h-full grid-rows-[49px_minmax(0,1fr)] overflow-hidden">
      <header class="flex min-w-0 items-center gap-[7px] border-b border-border bg-panel px-[9px] max-[700px]:gap-[3px]">
        <div class="flex min-w-[195px] items-center gap-[7px] max-[900px]:min-w-fit">
          <IconButton
            label="Toggle sidebar"
            icon={PanelLeft}
            active={sidebarOpen()}
            onClick={() => setSidebarOpen((value) => !value)}
          />
          <div class="flex min-w-0 items-center gap-2">
            <span class="grid size-[27px] shrink-0 place-items-center rounded-[7px] bg-primary-soft text-primary-strong">
              <GitBranch size={15} />
            </span>
            <div class="flex min-w-0 flex-col gap-px max-[900px]:hidden">
              <strong class="truncate text-[11px]">{repository().name}</strong>
              <span class="truncate text-[8.5px] text-faint">
                {repository().owner} / {repository().branch}
              </span>
            </div>
          </div>
        </div>
        <div class="flex items-center gap-0.5">
          <For each={toolbarActions}>
            {(action, index) => (
              <>
                {index() === 2 && <span class="mx-1 h-6 w-px bg-border" />}
                <button
                  class={`flex h-[38px] min-w-[43px] cursor-pointer flex-col items-center justify-center gap-0.5 rounded-md border-0 bg-transparent px-1.5 text-muted hover:bg-panel-hover hover:text-text max-[900px]:min-w-9 max-[900px]:[&>span]:hidden ${action.accent ? "text-blue" : ""} ${[0, 1, 5, 6].includes(index()) ? "max-[1100px]:hidden" : ""} ${index() >= 4 ? "max-[700px]:hidden" : ""}`}
                  title={action.label}
                >
                  <Dynamic component={action.icon} size={16} />
                  <span class="text-[8px]">{action.label}</span>
                </button>
              </>
            )}
          </For>
        </div>
        <div class="ml-auto max-[900px]:min-w-[115px] max-[700px]:min-w-[90px]">
          <TextInput compact value={search()} onChange={setSearch} placeholder="Search history…" />
        </div>
        <IconButton
          label="Toggle details panel"
          icon={PanelRight}
          active={detailsOpen()}
          onClick={() => setDetailsOpen((value) => !value)}
        />
        <IconButton label="Repository settings" icon={Settings} />
      </header>

      <div
        class={`relative grid min-h-0 min-w-0 overflow-hidden ${
          sidebarOpen()
            ? detailsOpen()
              ? "grid-cols-[220px_minmax(0,1fr)_326px] max-[1100px]:grid-cols-[210px_minmax(0,1fr)] max-[900px]:grid-cols-[52px_minmax(0,1fr)]"
              : "grid-cols-[220px_minmax(0,1fr)_0] max-[1100px]:grid-cols-[210px_minmax(0,1fr)] max-[900px]:grid-cols-[52px_minmax(0,1fr)]"
            : detailsOpen()
              ? "grid-cols-[0_minmax(0,1fr)_326px] max-[1100px]:grid-cols-[0_minmax(0,1fr)]"
              : "grid-cols-[0_minmax(0,1fr)_0] max-[1100px]:grid-cols-[0_minmax(0,1fr)]"
        }`}
      >
        <aside
          class={`relative flex min-h-0 min-w-0 flex-col overflow-hidden border-r border-border bg-panel transition-opacity duration-150 motion-reduce:transition-none max-[900px]:w-[52px] ${sidebarOpen() ? "" : "pointer-events-none opacity-0"}`}
        >
          <div class="pt-[9px] pr-2.5 pb-1.5 pl-2.5 max-[900px]:px-[7px] max-[900px]:py-2">
            <SegmentedControl
              value={sidebarMode()}
              options={["List", "Agents"]}
              onChange={setSidebarMode}
            />
          </div>
          <Show when={sidebarMode() === "List"} fallback={<AgentPlaceholder />}>
            <nav
              class="min-h-0 flex-1 overflow-y-auto pt-0.5 pr-[7px] pb-[35px] pl-[7px]"
              aria-label="Repository references"
            >
              <SidebarSection
                title="Workspace"
                icon={Folder}
                count={1}
                collapsed={false}
                onToggle={() => undefined}
                locked
              >
                <SidebarRow icon={File} label="Working directory" active count={6} tone="amber" />
              </SidebarSection>
              <SidebarSection
                title="Local branches"
                icon={GitBranch}
                count={localBranches.length}
                collapsed={false}
                onToggle={() => undefined}
                locked
              >
                <For each={localBranches}>
                  {(branch) => (
                    <SidebarRow
                      icon={GitBranch}
                      label={branch.name}
                      active={branch.current}
                      meta={
                        branch.ahead
                          ? `↑${branch.ahead}`
                          : branch.behind
                            ? `↓${branch.behind}`
                            : undefined
                      }
                    />
                  )}
                </For>
              </SidebarSection>
              <SidebarSection
                title="Remotes"
                icon={Cloud}
                count={remoteBranches.length}
                collapsed={collapsed().remotes}
                onToggle={() => toggleSection("remotes")}
              >
                <For each={remoteBranches}>
                  {(branch) => <SidebarRow icon={GitBranch} label={branch.name} />}
                </For>
              </SidebarSection>
              <SidebarSection
                title="Pull requests"
                icon={GitMerge}
                count={2}
                collapsed={collapsed().pullRequests}
                onToggle={() => toggleSection("pullRequests")}
              >
                <SidebarRow icon={GitMerge} label="#24 Polish desktop shell" meta="open" />
                <SidebarRow icon={GitMerge} label="#18 Theme tokens" meta="merged" />
              </SidebarSection>
              <SidebarSection
                title="Tags"
                icon={Tag}
                count={3}
                collapsed={collapsed().tags}
                onToggle={() => toggleSection("tags")}
              >
                <SidebarRow icon={Tag} label="v0.1.0" />
              </SidebarSection>
              <SidebarSection
                title="Stashes"
                icon={Inbox}
                count={1}
                collapsed={collapsed().stashes}
                onToggle={() => toggleSection("stashes")}
              >
                <SidebarRow icon={Archive} label="WIP: layout experiment" />
              </SidebarSection>
            </nav>
          </Show>
          <button class="absolute right-0 bottom-0 left-0 flex h-[34px] cursor-pointer items-center gap-[7px] border-0 border-t border-border-soft bg-panel px-3 text-[9px] text-faint hover:text-text-soft max-[900px]:justify-center max-[900px]:px-0 max-[900px]:[&>span]:hidden">
            <Settings size={15} />
            <span>Configure sidebar</span>
          </button>
        </aside>

        <main class="min-h-0 min-w-0 overflow-hidden bg-bg">
          <div class="grid h-[31px] min-w-[705px] grid-cols-[minmax(105px,.8fr)_82px_minmax(240px,2.2fr)_minmax(115px,.85fr)_100px_63px] items-center border-b border-border bg-bg-soft text-[8px] font-[720] tracking-[.055em] text-faint uppercase [&>span]:flex [&>span]:h-full [&>span]:items-center [&>span]:border-r [&>span]:border-border-soft [&>span]:px-[9px]">
            <span>Branch / tag</span>
            <span>Graph</span>
            <span>Commit message</span>
            <span>Author</span>
            <span>Date</span>
            <span>SHA</span>
          </div>
          <div
            class="h-[calc(100%_-_31px)] min-w-[705px] overflow-auto"
            role="listbox"
            aria-label="Commit history"
          >
            <Show
              when={visibleCommits().length}
              fallback={
                <div class="flex h-[250px] flex-col items-center justify-center gap-1.5 text-faint">
                  <Search size={22} />
                  <strong class="text-[11px] text-text-soft">No commits found</strong>
                  <span class="text-[9px]">Try a different message, author, or SHA.</span>
                </div>
              }
            >
              <For each={visibleCommits()}>
                {(commit) => (
                  <button
                    role="option"
                    aria-selected={selectedId() === commit.id ? "true" : "false"}
                    class={`group grid h-[47px] w-full min-w-[705px] cursor-pointer grid-cols-[minmax(105px,.8fr)_82px_minmax(240px,2.2fr)_minmax(115px,.85fr)_100px_63px] items-center border-0 border-b border-border-soft bg-transparent p-0 text-left text-muted hover:bg-panel-hover [&>span]:min-w-0 [&>span]:px-[9px] ${selectedId() === commit.id ? "bg-[linear-gradient(90deg,var(--primary-soft),color-mix(in_srgb,var(--primary-soft)_35%,transparent))] text-text-soft shadow-[inset_2px_0_var(--primary)]" : ""} ${commit.isWip ? "bg-[color-mix(in_srgb,var(--amber-soft)_35%,transparent)]" : ""}`}
                    onClick={() => setSelectedId(commit.id)}
                  >
                    <span class="flex gap-1 overflow-hidden">
                      <For each={commit.refs.slice(0, 2)}>
                        {(ref) => (
                          <span
                            class={`max-w-[78px] truncate rounded-sm border border-border bg-panel px-[5px] py-[3px] text-[8px] text-text-soft ${ref === "HEAD" || ref === "WIP" ? "border-[color-mix(in_srgb,var(--primary)_40%,var(--border))] bg-primary-soft text-primary-strong" : ref.startsWith("origin") ? "bg-blue-soft text-blue" : ""}`}
                          >
                            {ref}
                          </span>
                        )}
                      </For>
                    </span>
                    <span
                      class="flex h-full items-center font-mono text-[19px] font-bold tracking-[-5px] whitespace-pre [&>i]:w-[18px] [&>i]:not-italic"
                      aria-label={`Graph ${commit.graph.join(" ")}`}
                    >
                      <For each={commit.graph}>
                        {(cell, index) => (
                          <i class={["text-primary-strong", "text-blue", "text-mint"][index()]}>
                            {cell}
                          </i>
                        )}
                      </For>
                    </span>
                    <span class="flex min-w-0 items-center justify-between gap-[7px]">
                      <strong class="truncate text-[10.5px] font-[570] text-text">
                        {commit.message}
                      </strong>
                      <small
                        class={`items-center gap-[5px] text-[8px] group-hover:flex ${selectedId() === commit.id ? "flex" : "hidden"}`}
                      >
                        <span class="text-mint">+{commit.additions}</span>
                        <span class="text-coral">−{commit.deletions}</span>
                      </small>
                    </span>
                    <span class="flex items-center gap-[7px]">
                      <Avatar initials={commit.initials} color={commit.avatarColor} />
                      <span class="truncate text-[9.5px]">
                        {commit.author.split(" ").map((part, index) => (
                          <>
                            {index > 0 && " "}
                            <span>{part}</span>
                          </>
                        ))}
                      </span>
                    </span>
                    <span class="truncate text-[9px]">{commit.timestamp}</span>
                    <span class="font-mono text-[9px] text-faint">{commit.sha}</span>
                  </button>
                )}
              </For>
            </Show>
          </div>
        </main>

        <aside
          class={`min-h-0 min-w-0 overflow-hidden border-l border-border bg-panel transition-[opacity,transform] duration-150 motion-reduce:transition-none max-[1100px]:absolute max-[1100px]:top-0 max-[1100px]:right-0 max-[1100px]:bottom-0 max-[1100px]:z-[5] max-[1100px]:w-[340px] max-[1100px]:shadow-[-18px_0_40px_rgba(5,3,7,.25)] max-[700px]:w-[min(340px,calc(100%_-_52px))] ${detailsOpen() ? "" : "pointer-events-none opacity-0 max-[1100px]:translate-x-full"}`}
        >
          <CommitDetails commit={selectedCommit()} />
        </aside>
      </div>
    </div>
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

function SidebarRow(props: {
  icon: LucideIcon;
  label: string;
  active?: boolean;
  count?: number;
  meta?: string;
  tone?: "amber";
}) {
  return (
    <button
      class={`grid h-7 w-full cursor-pointer grid-cols-[16px_minmax(0,1fr)_auto_auto] items-center gap-[5px] rounded-[5px] border-0 bg-transparent pr-[7px] pl-[25px] text-left text-muted hover:bg-panel-hover hover:text-text-soft ${props.active ? "bg-primary-soft text-text [&>svg]:text-primary-strong" : ""}`}
      title={props.label}
    >
      <Dynamic component={props.icon} size={13} />
      <span class="truncate text-[10px]">{props.label}</span>
      {props.meta && <small class="text-[8px] text-blue">{props.meta}</small>}
      {props.count !== undefined && (
        <em
          class={`min-w-[17px] rounded-lg bg-panel-raised px-1 py-0.5 text-center text-[8px] not-italic ${props.tone === "amber" ? "bg-amber-soft text-amber" : "text-muted"}`}
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
      <span class="relative inline-flex h-8 w-[38px] items-center justify-center rounded-[45%_55%_53%_47%/59%_43%_57%_41%] bg-[linear-gradient(145deg,#bc8aef,#7861e6)] before:size-[3px] before:rounded-full before:bg-[#2b2032] before:shadow-[11px_0_#2b2032] before:content-['']">
        <i />
        <i />
      </span>
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

function CommitDetails(props: { commit: Commit }) {
  return (
    <div class="h-full min-w-[280px] overflow-y-auto">
      <div class="flex h-[38px] items-center justify-between border-b border-border py-0 pr-[9px] pl-[13px] text-[9px] font-[720] tracking-[.07em] text-muted uppercase">
        <span>{props.commit.isWip ? "Working directory" : "Commit details"}</span>
        <IconButton label="More commit actions" icon={Ellipsis} />
      </div>
      <Show when={props.commit.isWip} fallback={<CommitSummary commit={props.commit} />}>
        <div class="flex items-center gap-[13px] border-b border-border p-4">
          <span class="relative inline-flex h-11 w-[52px] shrink-0 items-center justify-center rounded-[51%_49%_43%_57%/57%_42%_58%_43%] bg-[linear-gradient(145deg,#bc8aef,#7861e6)] shadow-[0_8px_20px_rgba(122,76,170,.2)] before:size-1 before:rounded-full before:bg-[#2b2032] before:shadow-[14px_0_#2b2032] before:content-['']">
            <i />
            <i />
          </span>
          <div>
            <Badge tone="amber">WIP</Badge>
            <h2 class="mt-[7px] mb-1.5 text-sm leading-[1.35] tracking-[-.2px]">
              Uncommitted changes
            </h2>
            <p class="m-0 text-[9.5px] leading-[1.55] text-muted">
              6 files changed on <strong class="text-text-soft">main</strong>
            </p>
          </div>
        </div>
      </Show>
      <div class="border-b border-border px-2.5 py-3">
        <div class="flex items-center justify-between pt-0 pr-1 pb-2 pl-1">
          <div class="flex items-center gap-1.5">
            <File size={15} />
            <strong class="text-[10px]">Changed files</strong>
            <Badge>{props.commit.files.length}</Badge>
          </div>
          <button class="cursor-pointer border-0 bg-transparent text-[9px] text-primary-strong">
            Stage all
          </button>
        </div>
        <div class="flex flex-col gap-0.5">
          <For each={props.commit.files}>
            {(file) => (
              <button class="grid w-full cursor-pointer grid-cols-[18px_minmax(0,1fr)_auto] items-center gap-[7px] rounded-md border-0 bg-transparent p-[7px] text-left hover:bg-panel-hover">
                <span
                  class={`grid size-[17px] place-items-center rounded-sm font-mono text-[8px] font-bold ${
                    file.status === "added"
                      ? "bg-mint-soft text-mint"
                      : file.status === "deleted"
                        ? "bg-coral-soft text-coral"
                        : file.status === "renamed"
                          ? "bg-blue-soft text-blue"
                          : "bg-amber-soft text-amber"
                  }`}
                >
                  {fileStatusLabel[file.status]}
                </span>
                <span class="flex min-w-0 flex-col gap-0.5">
                  <strong class="truncate text-[9.5px] font-[540]">
                    {file.path.split("/").slice(-1)[0]}
                  </strong>
                  <small class="truncate text-[8px] text-faint">
                    {file.path.includes("/")
                      ? file.path.slice(0, file.path.lastIndexOf("/"))
                      : "root"}
                  </small>
                </span>
                <span class="flex gap-[5px] text-[8px]">
                  <em class="text-mint not-italic">+{file.additions}</em>
                  <b class="font-medium text-coral">−{file.deletions}</b>
                </span>
              </button>
            )}
          </For>
        </div>
      </div>
      <Show when={props.commit.isWip}>
        <div class="p-3.5">
          <label class="mb-2.5 flex flex-col gap-[5px] text-[9px] text-muted">
            <span class="flex justify-between">Commit message</span>
            <input
              class="w-full resize-y rounded-md border border-border bg-bg px-[9px] py-2 text-[9.5px] text-text outline-0 focus:border-primary focus:shadow-[0_0_0_3px_var(--primary-soft)]"
              placeholder="Summary of your changes"
            />
          </label>
          <label class="mb-2.5 flex flex-col gap-[5px] text-[9px] text-muted">
            <span class="flex justify-between">
              Description <small class="text-faint">optional</small>
            </span>
            <textarea
              class="w-full resize-y rounded-md border border-border bg-bg px-[9px] py-2 text-[9.5px] text-text outline-0 focus:border-primary focus:shadow-[0_0_0_3px_var(--primary-soft)]"
              placeholder="Add more context…"
              rows={3}
            />
          </label>
          <button class="flex h-8 w-full cursor-pointer items-center justify-center gap-[7px] rounded-[7px] border border-[color-mix(in_srgb,var(--primary)_50%,var(--border))] bg-[linear-gradient(135deg,#c18deb,#9d72d7)] text-[10px] font-[680] text-[#21152a] [&>kbd]:ml-auto [&>kbd]:pr-[7px] [&>kbd]:text-[8px] [&>kbd]:text-[rgba(34,20,42,.65)]">
            <GitCommitHorizontal size={15} />
            Commit changes <kbd>⌘ ↵</kbd>
          </button>
        </div>
      </Show>
    </div>
  );
}

function CommitSummary(props: { commit: Commit }) {
  return (
    <div class="border-b border-border p-4">
      <div class="flex items-center gap-[9px]">
        <Avatar initials={props.commit.initials} color={props.commit.avatarColor} size="md" />
        <div class="flex flex-col gap-0.5">
          <strong class="text-[10.5px]">{props.commit.author}</strong>
          <span class="text-[8.5px] text-faint">{props.commit.timestamp}</span>
        </div>
      </div>
      <h2 class="mt-3.5 mb-1.5 text-sm leading-[1.35] tracking-[-.2px]">{props.commit.message}</h2>
      {props.commit.description && (
        <p class="m-0 text-[9.5px] leading-[1.55] text-muted">{props.commit.description}</p>
      )}
      <div class="mt-3 flex w-max items-center overflow-hidden rounded-[5px] border border-border-soft">
        <code class="bg-bg px-[7px] py-1 text-[8.5px] text-text-soft">{props.commit.sha}</code>
        <button
          class="grid h-[23px] w-6 cursor-pointer place-items-center border-0 border-l border-border-soft bg-panel-raised p-0 text-faint"
          aria-label="Copy commit SHA"
        >
          <Copy size={13} />
        </button>
      </div>
      <div class="mt-[13px] flex items-center gap-2 text-[9px] text-muted">
        <span class="mr-auto">
          <strong>{props.commit.files.length}</strong> files changed
        </span>
        <em class="text-mint not-italic">+{props.commit.additions}</em>
        <b class="font-medium text-coral">−{props.commit.deletions}</b>
      </div>
    </div>
  );
}
