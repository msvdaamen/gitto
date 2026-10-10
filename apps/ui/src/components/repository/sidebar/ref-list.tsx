import type { Ref } from "@gitto/git/types";
import Archive from "lucide-solid/icons/archive";
import Cloud from "lucide-solid/icons/cloud";
import FolderGit2 from "lucide-solid/icons/folder-git-2";
import GitBranch from "lucide-solid/icons/git-branch";
import Inbox from "lucide-solid/icons/inbox";
import Tag from "lucide-solid/icons/tag";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import { createMemo, createSignal, Match, Show, Switch } from "solid-js";
import type { JSX } from "solid-js";

import { EmptyState } from "@/components/ui/empty-state";
import { useSwitchBranch } from "@/git/queries/branch";
import { useRefs } from "@/git/queries/refs";
import { useStashes } from "@/git/queries/stash";
import { useWorktrees } from "@/git/queries/worktree";
import { buildRefTree, flattenRefTree } from "@/git/ref-tree";
import type { RefFolder, RefLeaf, RefTreeRow } from "@/git/ref-tree";
import { branchName } from "@/git/status";
import { canOpenWorktree, describeWorktree, worktreeOn } from "@/git/worktree";
import { useCollapsed } from "@/hooks/collapsed";
import { useRelativeTime } from "@/hooks/relative-time";

import { BranchMenu, useBranchMenuOpenFor, useOpenWorktreeFromBranch } from "../branch-menu";
import { SidebarFolder, SidebarRow } from "./sidebar-row";
import { SidebarSection } from "./sidebar-section";
import { StashMenu } from "./stash-menu";
import { WorktreeMenu } from "./worktree-menu";

/**
 * The sidebar's sections: branches, remotes, tags, stashes, worktrees.
 * Double-clicking a branch switches to it; for a remote one, to the local branch tracking it.
 * Right-clicking one opens a menu of what can be done with it, like creating a branch from it;
 * right-clicking a stash, one to pop or delete it. A branch checked out in another worktree
 * says so, and double-clicking it opens that worktree instead, as git won't switch to it here.
 * Double-clicking a worktree opens it, as a repository of its own; right-clicking one opens a
 * menu to open or remove it.
 */
export function RefList(props: { repositoryId: string }) {
  const refs = useRefs(() => props.repositoryId);
  const stashes = useStashes(() => props.repositoryId);
  const worktrees = useWorktrees(() => props.repositoryId);
  const switchBranch = useSwitchBranch(() => props.repositoryId);
  const ago = useRelativeTime();
  // The stash whose menu is open.
  const [menuFor, setMenuFor] = createSignal<string>();
  // The worktree whose menu is open.
  const [worktreeMenuFor, setWorktreeMenuFor] = createSignal<string>();

  const ofKind = (kind: Ref["kind"]) => (refs.data ?? []).filter((ref) => ref.kind === kind);
  const localBranches = createMemo(() => ofKind("local"));
  const remoteBranches = createMemo(() => ofKind("remote"));
  const tags = createMemo(() => ofKind("tag"));

  // Sections and ref folders share one saved state; folder ids are full ref paths like
  // `refs/heads/feature`, so they can't clash with the section ids.
  const collapsed = useCollapsed(() => props.repositoryId, { tags: true, stashes: true });
  // The worktrees other than the one on show, whose files are these.
  const otherWorktrees = createMemo(() =>
    (worktrees.isSuccess ? worktrees.data : []).filter((worktree) => !worktree.current),
  );
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

  /**
   * A local or remote branch's line in its tree, with its menu (see `BranchMenu`). One checked
   * out in another worktree has a folder after its name, and says which in its tooltip.
   */
  const branchRow = (branch: () => Ref, label: () => string, depth: () => number) => {
    const openFor = useBranchMenuOpenFor();
    const openWorktree = useOpenWorktreeFromBranch();
    const worktree = () => worktreeOn(otherWorktrees(), branch().fullName);
    return (
      <SidebarRow
        data-branch={branch().fullName}
        icon={GitBranch}
        label={label()}
        title={worktree() ? `${branch().name}\nChecked out in ${worktree()!.path}` : branch().name}
        depth={depth()}
        active={branch().current}
        highlighted={openFor() === branch().fullName}
        tag={
          worktree() ? { icon: FolderGit2, label: `Checked out in ${worktree()!.name}` } : undefined
        }
        meta={
          branch().ahead
            ? `↑${branch().ahead}`
            : branch().behind
              ? `↓${branch().behind}`
              : undefined
        }
        // The toolbar shows a switch running, and why it failed; the worktrees' section, opening
        // the worktree.
        onDblClick={() => {
          const there = worktree();
          if (!there) switchBranch.run(branch().fullName);
          else if (canOpenWorktree(there)) openWorktree(there.path);
        }}
      />
    );
  };

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
      <BranchMenu repositoryId={props.repositoryId}>
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
          title="Tags"
          scrollId="sidebar-tags"
          icon={Tag}
          count={tags().length}
          {...collapsible("tags")}
          items={tags()}
        >
          {(tag) => <SidebarRow icon={Tag} label={tag().name} />}
        </SidebarSection>
      </BranchMenu>
      <StashMenu repositoryId={props.repositoryId} onOpenFor={setMenuFor}>
        {(section) => (
          <SidebarSection
            ref={section.ref}
            title="Stashes"
            scrollId="sidebar-stashes"
            icon={Inbox}
            busy={section.busy()}
            count={stashes.data?.length ?? 0}
            {...collapsible("stashes")}
            items={stashes.data ?? []}
          >
            {(stash) => (
              <SidebarRow
                data-stash={stash().sha}
                icon={Archive}
                label={stash().message}
                title={`${stash().message}\nStashed ${ago(stash().createdAt).toLowerCase()}`}
                highlighted={menuFor() === stash().sha}
              />
            )}
          </SidebarSection>
        )}
      </StashMenu>
      <WorktreeMenu repositoryId={props.repositoryId} onOpenFor={setWorktreeMenuFor}>
        {(section) => (
          <SidebarSection
            ref={section.ref}
            title="Worktrees"
            scrollId="sidebar-worktrees"
            icon={FolderGit2}
            busy={section.busy()}
            count={worktrees.data?.length ?? 0}
            {...collapsible("worktrees")}
            items={worktrees.data ?? []}
          >
            {(worktree) => (
              <SidebarRow
                data-worktree={worktree().path}
                icon={FolderGit2}
                label={worktree().name}
                title={describeWorktree(worktree())}
                active={worktree().current}
                highlighted={worktreeMenuFor() === worktree().path}
                faded={worktree().prunable !== null}
                meta={
                  worktree().branch
                    ? branchName(worktree().branch!)
                    : worktree().bare
                      ? "bare"
                      : "detached"
                }
                // The section shows it running, and why it failed.
                onDblClick={() => section.open(worktree())}
              />
            )}
          </SidebarSection>
        )}
      </WorktreeMenu>
    </>
  );
}
