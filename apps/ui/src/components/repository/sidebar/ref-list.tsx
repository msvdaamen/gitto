import type { Ref } from "@gitto/git/types";
import Archive from "lucide-solid/icons/archive";
import Cloud from "lucide-solid/icons/cloud";
import File from "lucide-solid/icons/file";
import Folder from "lucide-solid/icons/folder";
import GitBranch from "lucide-solid/icons/git-branch";
import GitMerge from "lucide-solid/icons/git-merge";
import Inbox from "lucide-solid/icons/inbox";
import Tag from "lucide-solid/icons/tag";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import { createEffect, createMemo, createSignal, Match, on, Show, Switch } from "solid-js";
import type { JSX } from "solid-js";

import { EmptyState } from "@/components/ui/empty-state";
import { useSwitchBranch } from "@/git/queries/branch";
import { useRefs } from "@/git/queries/refs";
import { useStashes } from "@/git/queries/stash";
import { useStatus } from "@/git/queries/status";
import { buildRefTree, flattenRefTree } from "@/git/ref-tree";
import type { RefFolder, RefLeaf, RefTreeRow } from "@/git/ref-tree";
import { useCollapsed } from "@/hooks/collapsed";
import { relativeTime } from "@/lib/format";

import { BranchMenu } from "./branch-menu";
import { CreateBranchDialog } from "./create-branch-dialog";
import { SidebarFolder, SidebarRow } from "./sidebar-row";
import { SidebarSection } from "./sidebar-section";

// Placeholders until pull requests are loaded from the repository.
const PULL_REQUESTS = [
  { label: "#24 Polish desktop shell", meta: "open" },
  { label: "#18 Theme tokens", meta: "merged" },
];

/**
 * The sidebar's sections: the working directory, branches, remotes, pull requests, tags, stashes.
 * Double-clicking a branch switches to it; for a remote one, to the local branch tracking it.
 * Right-clicking one opens a menu of what can be done with it, like creating a branch from it.
 */
export function RefList(props: { repositoryId: string }) {
  const status = useStatus(() => props.repositoryId);
  const refs = useRefs(() => props.repositoryId);
  const stashes = useStashes(() => props.repositoryId);
  const switchBranch = useSwitchBranch(() => props.repositoryId);
  // The branch whose menu is open, and the one a new one is being named to be created from.
  const [menuFor, setMenuFor] = createSignal<string>();
  const [branchFrom, setBranchFrom] = createSignal<Ref>();
  createEffect(
    on(
      () => props.repositoryId,
      () => setBranchFrom(undefined),
      { defer: true },
    ),
  );

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

  /**
   * A ref tree's line: a collapsible folder, or a ref drawn by `ref`. Which of the two can change,
   * as lines are rendered by position: collapsing a folder moves the ones below it up.
   */
  const treeRow = (
    row: () => RefTreeRow,
    ref: (ref: () => Ref, label: () => string, depth: () => number) => JSX.Element,
  ) => {
    const depth = () => row().depth;
    // The line's folder or ref; the last one while the line turns into the other, rather than a
    // <Match>'s narrowed value. Switching repositories happens in a transition, which removes what
    // was rendered for the line only once it's over, and its effects can still run in between:
    // reading the narrowed value there would throw.
    const folder = createMemo<RefFolder | undefined>((last) => {
      const { node } = row();
      return node.type === "folder" ? node : last;
    });
    const leaf = createMemo<RefLeaf | undefined>((last) => {
      const { node } = row();
      return node.type === "ref" ? node : last;
    });
    return (
      <Switch>
        <Match when={row().node.type === "folder"}>
          <SidebarFolder
            name={folder()!.name}
            count={folder()!.count}
            depth={depth()}
            collapsed={collapsed.isCollapsed(folder()!.path)}
            onToggle={() => collapsed.toggle(folder()!.path)}
          />
        </Match>
        <Match when={row().node.type === "ref"}>
          {ref(
            () => leaf()!.ref,
            () => leaf()!.name,
            depth,
          )}
        </Match>
      </Switch>
    );
  };

  /** A local or remote branch's line in its tree, with its menu (see `BranchMenu`). */
  const branchRow = (branch: () => Ref, label: () => string, depth: () => number) => (
    <SidebarRow
      data-branch={branch().fullName}
      icon={GitBranch}
      label={label()}
      title={branch().name}
      depth={depth()}
      active={branch().current}
      highlighted={menuFor() === branch().fullName}
      meta={
        branch().ahead ? `↑${branch().ahead}` : branch().behind ? `↓${branch().behind}` : undefined
      }
      // The toolbar shows it running, and why it failed.
      onDblClick={() => switchBranch.run(branch().fullName)}
    />
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
      <Show when={refs.error} keyed>
        {(error) => (
          <EmptyState
            icon={TriangleAlert}
            title="Couldn't load branches"
            tone="error"
            class="h-[150px] shrink-0"
          >
            {error.message}
          </EmptyState>
        )}
      </Show>
      <BranchMenu
        branch={(fullName) => refs.data?.find((ref) => ref.fullName === fullName)}
        switching={switchBranch.isPending()}
        onOpenFor={setMenuFor}
        onCreateBranch={setBranchFrom}
      >
        <SidebarSection
          title="Workspace"
          scrollId="sidebar-workspace"
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
              count={status.data?.counts.files ?? 0}
              tone="amber"
            />
          )}
        </SidebarSection>
        <SidebarSection
          title="Local branches"
          scrollId="sidebar-local-branches"
          icon={GitBranch}
          count={localBranches().length}
          {...collapsible("local")}
          items={localRows()}
        >
          {(row) => treeRow(row, branchRow)}
        </SidebarSection>
        <SidebarSection
          title="Remotes"
          scrollId="sidebar-remotes"
          icon={Cloud}
          count={remoteBranches().length}
          {...collapsible("remotes")}
          items={remoteRows()}
        >
          {(row) => treeRow(row, branchRow)}
        </SidebarSection>
        <SidebarSection
          title="Pull requests"
          scrollId="sidebar-pull-requests"
          icon={GitMerge}
          count={PULL_REQUESTS.length}
          {...collapsible("pullRequests")}
          items={PULL_REQUESTS}
        >
          {(pr) => <SidebarRow icon={GitMerge} label={pr().label} meta={pr().meta} />}
        </SidebarSection>
        <SidebarSection
          title="Tags"
          scrollId="sidebar-tags"
          icon={Tag}
          count={tags().length}
          {...collapsible("tags")}
          items={tags()}
        >
          {(tag) => <SidebarRow icon={Tag} label={tag().name} />}
        </SidebarSection>
        <SidebarSection
          title="Stashes"
          scrollId="sidebar-stashes"
          icon={Inbox}
          count={stashes.data?.length ?? 0}
          {...collapsible("stashes")}
          items={stashes.data ?? []}
        >
          {(stash) => (
            <SidebarRow
              icon={Archive}
              label={stash().message}
              title={`${stash().message}\nStashed ${relativeTime(stash().createdAt).toLowerCase()}`}
            />
          )}
        </SidebarSection>
      </BranchMenu>
      <CreateBranchDialog
        repositoryId={props.repositoryId}
        from={branchFrom()}
        onClose={() => setBranchFrom(undefined)}
      />
    </>
  );
}
