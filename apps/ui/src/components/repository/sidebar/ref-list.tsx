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
import { createMemo, Show } from "solid-js";
import type { JSX } from "solid-js";

import { EmptyState } from "@/components/ui/empty-state";
import { useRefs } from "@/git/queries/refs";
import { useStatus } from "@/git/queries/status";
import { buildRefTree, flattenRefTree } from "@/git/ref-tree";
import type { RefTreeRow } from "@/git/ref-tree";
import { useCollapsed } from "@/hooks/collapsed";

import { SidebarFolder, SidebarRow } from "./sidebar-row";
import { SidebarSection } from "./sidebar-section";

// Placeholders until pull requests and stashes are loaded from the repository.
const PULL_REQUESTS = [
  { label: "#24 Polish desktop shell", meta: "open" },
  { label: "#18 Theme tokens", meta: "merged" },
];
const STASHES = [{ label: "WIP: layout experiment" }];

/** The sidebar's sections: the working directory, branches, remotes, pull requests, tags, stashes. */
export function RefList(props: { repositoryId: string }) {
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
