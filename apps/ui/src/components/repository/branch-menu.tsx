import type { ChangedFile, Uncommitted } from "@gitto/git/types";
import { useQueryClient } from "@tanstack/solid-query";
import FolderOpen from "lucide-solid/icons/folder-open";
import FolderPlus from "lucide-solid/icons/folder-plus";
import GitBranchPlus from "lucide-solid/icons/git-branch-plus";
import GitMerge from "lucide-solid/icons/git-merge";
import {
  createContext,
  createEffect,
  createSignal,
  on,
  Show,
  useContext,
  type Accessor,
  type JSX,
} from "solid-js";

import { ContextMenuItem } from "@/components/ui/context-menu";
import { useMergeBranch, useMergeBranchState, useSwitchBranchState } from "@/git/queries/branch";
import { gitKeys } from "@/git/queries/keys";
import { useOperationInProgress } from "@/git/queries/progress";
import { firstToResolve, selectConflicted, useStatus } from "@/git/queries/status";
import { useUnsuspendedData } from "@/git/queries/unsuspended";
import { useOpenWorktree, useWorktrees } from "@/git/queries/worktree";
import { branchName, headLabel, isCheckedOut } from "@/git/status";
import { canOpenWorktree, worktreeOn } from "@/git/worktree";

import { AddWorktreeDialog } from "./add-worktree-dialog";
import { CreateBranchDialog } from "./create-branch-dialog";
import { RowMenu } from "./row-menu";

/** What the branch menus share, from their `BranchMenuProvider`. */
interface BranchActions {
  /** Begins creating a branch from another, by its full ref name, asking for the new one's name. */
  createFrom: (from: string) => void;
  /** Merges a branch, by its full ref name, into the checked-out one, `into`. */
  merge: (ref: string, into: string) => void;
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
 * the dialog placing a worktree, likewise, merging, and opening a worktree. A merge that stops at
 * conflicts opens the first conflicted file to resolve them.
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
  // The branch a new one is being named to be created from, by its full ref name.
  const [from, setFrom] = createSignal<string>();
  // The branch a worktree is being placed for, by its full ref name.
  const [worktreeFor, setWorktreeFor] = createSignal<string>();
  createEffect(
    on(
      () => props.repositoryId,
      () => {
        setFrom(undefined);
        setWorktreeFor(undefined);
      },
      { defer: true },
    ),
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
 * isn't offered for that one itself, and checking it out in a new worktree, or opening the one
 * it's checked out in. Neither of the first two is possible while a switch of branches or a merge
 * is running. Merging is also disabled while HEAD is detached, or an operation (a merge, a
 * rebase…) or conflicts are under way, as git would refuse. Needs a `BranchMenuProvider` around it.
 */
export function BranchMenu(props: { repositoryId: string; children: JSX.Element }) {
  const actions = useContext(BranchActions);
  if (!actions) throw new Error("A BranchMenu needs a BranchMenuProvider around it");
  const switching = useSwitchBranchState(() => props.repositoryId);
  const merging = useMergeBranchState(() => props.repositoryId);
  const status = useUnsuspendedData(useStatus(() => props.repositoryId));
  const operation = useUnsuspendedData(useOperationInProgress(() => props.repositoryId));
  const worktrees = useUnsuspendedData(useWorktrees(() => props.repositoryId));
  const [openFor, setOpenFor] = createSignal<string>();
  /** The other worktree the branch is checked out in, if one that can be opened is. */
  const worktreeOf = (branch: string) => {
    const worktree = worktreeOn(worktrees(), branch);
    return worktree && canOpenWorktree(worktree) ? worktree : undefined;
  };

  const busy = () => switching.isPending() || merging.isPending();
  /** The branch to merge into: the one checked out; `undefined` with HEAD detached. */
  const into = () => {
    const head = status()?.head;
    return head && head.kind !== "detached" ? head.name : undefined;
  };
  const mayMerge = () =>
    into() !== undefined && !status()?.counts.conflicted && operation() === null && !busy();

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
          </>
        )}
      >
        {props.children}
      </RowMenu>
    </OpenFor.Provider>
  );
}
