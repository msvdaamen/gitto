import { remoteOf } from "@gitto/git/remote-name";
import type { ChangedFile, Uncommitted } from "@gitto/git/types";
import { useQueryClient } from "@tanstack/solid-query";
import FolderOpen from "lucide-solid/icons/folder-open";
import FolderPlus from "lucide-solid/icons/folder-plus";
import GitBranchPlus from "lucide-solid/icons/git-branch-plus";
import GitMerge from "lucide-solid/icons/git-merge";
import Trash from "lucide-solid/icons/trash";
import { createContext, createSignal, Show, useContext, type Accessor, type JSX } from "solid-js";

import { ContextMenuItem } from "@/components/ui/context-menu";
import { useBranchesChanging, useDeleteBranch, useMergeBranch } from "@/git/queries/branch";
import { gitKeys } from "@/git/queries/keys";
import { useOperationInProgress } from "@/git/queries/progress";
import { useRefs } from "@/git/queries/refs";
import { useRemotes } from "@/git/queries/remote";
import { firstToResolve, selectConflicted, useStatus } from "@/git/queries/status";
import { useUnsuspendedData } from "@/git/queries/unsuspended";
import { useOpenWorktree, useWorktrees } from "@/git/queries/worktree";
import { branchName, headLabel, isCheckedOut } from "@/git/status";
import { canOpenWorktree, trackingBranches, worktreeOn } from "@/git/worktree";
import { useResetOn } from "@/hooks/reset-on";

import { AddWorktreeDialog } from "./add-worktree-dialog";
import { CreateBranchDialog } from "./create-branch-dialog";
import { DeleteBranchDialog, type BranchDeletion } from "./delete-branch-dialog";
import { RowMenu } from "./row-menu";

/** What the branch menus share, from their `BranchMenuProvider`. */
interface BranchActions {
  /** Begins creating a branch from another, by its full ref name, asking for the new one's name. */
  createFrom: (from: string) => void;
  /** Merges a branch, by its full ref name, into the checked-out one, `into`. */
  merge: (ref: string, into: string) => void;
  /** Begins deleting a branch, asking first. */
  deleteBranch: (deletion: BranchDeletion) => void;
  /** Begins adding a worktree for a branch, by its full ref name, asking where to put it. */
  addWorktreeFor: (branch: string) => void;
  /**
   * Opens the worktree at `path` in Gitto, as a repository of its own. The worktrees' section in
   * the sidebar shows it running, and why it failed (see `WorktreeMenu`).
   */
  openWorktree: (path: string) => void;
  /** Whether a worktree is being opened. */
  openingWorktree: () => boolean;
}

const BranchActions = createContext<BranchActions>();

/** The full ref name of the branch whose menu is open, if any. */
const OpenFor = createContext<Accessor<string | undefined>>(() => undefined);

/**
 * What the branch menus in `children` (see `BranchMenu`) share: the dialog naming a branch to
 * create, so a name kept to fix after a failure is there whichever menu it's opened from again,
 * the dialog placing a worktree, likewise, merging, the dialog asking before deleting a branch, and
 * opening a worktree. A merge that stops at conflicts opens the first conflicted file to resolve
 * them.
 */
export function BranchMenuProvider(props: {
  repositoryId: string;
  /** Opens a conflicted file's conflicts, as a merge that stopped at them asks. */
  onResolve?: (file: ChangedFile) => void;
  /**
   * What merging waits for, as it rewrites files in the working tree: the edits to a file on show
   * saved. Resolves to whether to go on.
   */
  beforeChange?: () => Promise<boolean>;
  children: JSX.Element;
}) {
  const queryClient = useQueryClient();
  const merging = useMergeBranch(() => props.repositoryId);
  const opening = useOpenWorktree(() => props.repositoryId);
  const deleting = useDeleteBranch(() => props.repositoryId);
  // The branch a new one is being named to be created from, by its full ref name.
  const [from, setFrom] = createSignal<string>();
  // The branch a worktree is being placed for, by its full ref name.
  const [worktreeFor, setWorktreeFor] = createSignal<string>();
  // The branch being asked about deleting.
  const [deletion, setDeletion] = createSignal<BranchDeletion>();
  useResetOn(
    () => props.repositoryId,
    () => {
      setFrom(undefined);
      setWorktreeFor(undefined);
      setDeletion(undefined);
    },
  );

  const merge = async (ref: string, into: string) => {
    const id = props.repositoryId;
    // Not once another repository is on show: the branch is this one's.
    if (!(await (props.beforeChange?.() ?? true)) || id !== props.repositoryId) return;
    merging.run(
      { ref, into },
      {
        onSuccess: (outcome) => {
          if (outcome !== "conflicts" || id !== props.repositoryId) return;
          // The status has reloaded by now (see `useMergeBranch`).
          const status = queryClient.getQueryData<Uncommitted>(gitKeys.status(id));
          const file = status && firstToResolve(selectConflicted(status));
          if (file) props.onResolve?.(file);
        },
      },
    );
  };

  return (
    <BranchActions.Provider
      value={{
        createFrom: setFrom,
        merge: (ref, into) => void merge(ref, into),
        deleteBranch: setDeletion,
        addWorktreeFor: setWorktreeFor,
        openWorktree: (path) => opening.run(path),
        openingWorktree: () => opening.isPending(),
      }}
    >
      {props.children}
      <CreateBranchDialog
        repositoryId={props.repositoryId}
        from={from()}
        onClose={() => setFrom(undefined)}
      />
      <AddWorktreeDialog
        repositoryId={props.repositoryId}
        branch={worktreeFor()}
        onClose={() => setWorktreeFor(undefined)}
      />
      <DeleteBranchDialog
        deletion={deletion()}
        onCancel={() => setDeletion(undefined)}
        onDelete={(branch) => {
          setDeletion(undefined);
          // The toolbar shows it running, and why it failed.
          deleting.run(branch.ref);
        }}
      />
    </BranchActions.Provider>
  );
}

/**
 * The full ref name of the branch whose menu is open, if any, for its element to show which one
 * it's for (see `BranchMenu`).
 */
export function useBranchMenuOpenFor() {
  return useContext(OpenFor);
}

/**
 * Opens a worktree in Gitto, by its path, from a `BranchMenuProvider`: for a branch's element to
 * open the worktree the branch is checked out in, rather than switching to it, which git refuses.
 */
export function useOpenWorktreeFromBranch(): (path: string) => void {
  const actions = useContext(BranchActions);
  return (path) => actions?.openWorktree(path);
}

/**
 * The menu of what can be done with a branch, opened by right-clicking an element in `children`
 * marked with its full ref name as `data-branch`, like a row in the sidebar or a label in the
 * history (see `RowMenu`): creating a branch from it, merging it into the checked-out one, which
 * isn't offered for that one itself, checking it out in a new worktree, or opening the one it's
 * checked out in, and deleting it, asking first. Deleting a local branch deletes it here only;
 * deleting a remote one deletes it on its remote, and the local branches tracking it with it.
 * None of creating, merging or deleting is possible while another change of the branches (a
 * switch, a merge, a deletion…) is running. Merging is also disabled while HEAD is detached, or an
 * operation (a merge, a rebase…) or conflicts are under way, and deleting while the branch, or for
 * a remote one a local branch tracking it, is checked out, here or in another worktree, as git
 * would refuse. (Nor will it delete one being rebased or bisected, which only it tells.)
 * Needs a `BranchMenuProvider` around it.
 */
export function BranchMenu(props: { repositoryId: string; children: JSX.Element }) {
  const actions = useContext(BranchActions);
  if (!actions) throw new Error("A BranchMenu needs a BranchMenuProvider around it");
  const busy = useBranchesChanging(() => props.repositoryId);
  const status = useUnsuspendedData(useStatus(() => props.repositoryId));
  const operation = useUnsuspendedData(useOperationInProgress(() => props.repositoryId));
  const worktrees = useUnsuspendedData(useWorktrees(() => props.repositoryId));
  const refs = useUnsuspendedData(useRefs(() => props.repositoryId));
  const remotes = useUnsuspendedData(useRemotes(() => props.repositoryId));
  const [openFor, setOpenFor] = createSignal<string>();
  /** The other worktree the branch is checked out in, if one that can be opened is. */
  const worktreeOf = (branch: string) => {
    const worktree = worktreeOn(worktrees(), branch);
    return worktree && canOpenWorktree(worktree) ? worktree : undefined;
  };

  /** The branch to merge into: the one checked out; `undefined` with HEAD detached. */
  const into = () => {
    const head = status()?.head;
    return head && head.kind !== "detached" ? head.name : undefined;
  };
  const mayMerge = () =>
    into() !== undefined && !status()?.counts.conflicted && operation() === null && !busy();

  /** Whether the local branch `ref` is checked out, here or in another worktree. */
  const checkedOut = (ref: string) => {
    const head = status()?.head;
    return (!!head && isCheckedOut(head, ref)) || worktreeOn(worktrees(), ref) !== undefined;
  };
  /**
   * What deleting the branch `ref` deletes with it, or keeps (see `BranchDeletion`); `undefined`
   * if git won't delete it, as it, or a local branch tracking it, is checked out, or until that's
   * known: the branches, the status and the worktrees have loaded. Until then, the dialog couldn't
   * say which local branches go with a remote one.
   */
  const deletion = (ref: string): BranchDeletion | undefined => {
    const all = refs();
    if (!all || !status() || !worktrees()) return undefined;
    if (!ref.startsWith("refs/remotes/")) {
      if (checkedOut(ref)) return undefined;
      const branch = all.find((candidate) => candidate.fullName === ref);
      // Its remote branch, if it tracks one that's there, rather than another local one.
      const upstream = branch?.upstream ?? null;
      const remote = all.some(
        (candidate) => candidate.kind === "remote" && candidate.name === upstream,
      );
      return {
        ref,
        kind: "local",
        upstream: remote ? upstream : null,
        ahead: remote ? (branch?.ahead ?? 0) : 0,
      };
    }
    const tracking = trackingBranches(all, ref);
    if (tracking.some((local) => checkedOut(local.fullName))) return undefined;
    return {
      ref,
      kind: "remote",
      // Taken to be there until the remotes have loaded, so it isn't played down.
      onRemote: !remotes() || remoteOf(remotes()!, branchName(ref)) !== undefined,
      tracking: tracking.map((local) => ({ name: local.name, ahead: local.ahead })),
    };
  };

  return (
    <OpenFor.Provider value={openFor}>
      <RowMenu
        repositoryId={props.repositoryId}
        attribute="branch"
        item={(fullName) => fullName}
        onOpenFor={setOpenFor}
        actions={(branch) => (
          <>
            <ContextMenuItem
              icon={GitBranchPlus}
              label="Create branch…"
              disabled={busy()}
              onSelect={() => actions.createFrom(branch())}
            />
            <Show when={status()?.head}>
              {(head) => (
                <Show when={!isCheckedOut(head(), branch())}>
                  <ContextMenuItem
                    icon={GitMerge}
                    label={`Merge ${branchName(branch())} into ${headLabel(head())}`}
                    disabled={!mayMerge()}
                    onSelect={() => {
                      const target = into();
                      if (target !== undefined) actions.merge(branch(), target);
                    }}
                  />
                </Show>
              )}
            </Show>
            <Show
              when={worktreeOf(branch())}
              fallback={
                <ContextMenuItem
                  icon={FolderPlus}
                  label="Add worktree…"
                  disabled={busy()}
                  onSelect={() => actions.addWorktreeFor(branch())}
                />
              }
            >
              {(worktree) => (
                <ContextMenuItem
                  icon={FolderOpen}
                  label={`Open worktree ${worktree().name}`}
                  disabled={actions.openingWorktree()}
                  onSelect={() => actions.openWorktree(worktree().path)}
                />
              )}
            </Show>
            <ContextMenuItem
              icon={Trash}
              label={`Delete ${branchName(branch())}…`}
              disabled={busy() || !deletion(branch())}
              onSelect={() => {
                const branchDeletion = deletion(branch());
                if (branchDeletion) actions.deleteBranch(branchDeletion);
              }}
            />
          </>
        )}
      >
        {props.children}
      </RowMenu>
    </OpenFor.Provider>
  );
}
