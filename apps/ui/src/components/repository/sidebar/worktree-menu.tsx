import type { Worktree } from "@gitto/git/types";
import FolderOpen from "lucide-solid/icons/folder-open";
import FolderX from "lucide-solid/icons/folder-x";
import { createEffect, createSignal, on, type JSX } from "solid-js";

import { ContextMenuItem } from "@/components/ui/context-menu";
import { FailurePopover } from "@/components/ui/failure-popover";
import { useWorktreeActions, useWorktrees } from "@/git/queries/worktree";
import { canOpenWorktree } from "@/git/worktree";

import { RowMenu } from "../row-menu";
import { RemoveWorktreeDialog } from "./remove-worktree-dialog";

/** What the worktrees' section is given by their menu (see `WorktreeMenu`). */
export interface WorktreeSectionFeedback {
  /** Takes the section's element, which why an action failed is shown beside. */
  ref: (element: HTMLElement) => void;
  /** Whether a worktree is being added, opened or removed. */
  busy: () => boolean;
  /** Opens a worktree in Gitto, e.g. the one whose row was double-clicked (see `WorktreeMenu`). */
  open: (worktree: Worktree) => void;
}

/**
 * The menu of what can be done with a worktree, opened by right-clicking its row in the section
 * `children` renders: one marked with its path as `data-worktree` (see `RowMenu`). Opening it
 * adds it to Gitto's repositories, as a repository of its own, and shows it; not for the one on
 * show, a bare one, which has no files, or one whose folder is gone. Removing it deletes its
 * folder, asking first, as its uncommitted changes are lost; not the main worktree, nor the one
 * on show. The section shows each running, and why it failed beside it, as it does a worktree
 * being added or opened from a branch's menu (see `AddWorktreeDialog` and `BranchMenu`).
 */
export function WorktreeMenu(props: {
  repositoryId: string;
  /** Told which worktree's menu is open, by its path, if any, to show which one it's for. */
  onOpenFor: (path: string | undefined) => void;
  children: (section: WorktreeSectionFeedback) => JSX.Element;
}) {
  const worktrees = useWorktrees(() => props.repositoryId);
  const { add, remove, open: opening, isPending } = useWorktreeActions(() => props.repositoryId);
  // The worktree being asked about removing, and the worktrees' section.
  const [removing, setRemoving] = createSignal<Worktree>();
  const [section, setSection] = createSignal<HTMLElement>();
  // A worktree of one repository isn't one of another.
  createEffect(
    on(
      () => props.repositoryId,
      () => setRemoving(undefined),
      { defer: true },
    ),
  );

  const open = (worktree: Worktree) => {
    if (canOpenWorktree(worktree)) opening.run(worktree.path);
  };

  // One at a time, the removal's first, then the addition's: each can have failed, one after the
  // other.
  const failure = () =>
    remove.error()
      ? { title: "Remove worktree", error: remove.error(), dismiss: () => remove.dismiss() }
      : add.error()
        ? { title: "Add worktree", error: add.error(), dismiss: () => add.dismiss() }
        : { title: "Open worktree", error: opening.error(), dismiss: () => opening.dismiss() };

  return (
    <>
      <RowMenu
        repositoryId={props.repositoryId}
        attribute="worktree"
        item={(path) => worktrees.data?.find((worktree) => worktree.path === path)}
        onOpenFor={(path) => props.onOpenFor(path)}
        actions={(worktree) => (
          <>
            <ContextMenuItem
              icon={FolderOpen}
              label="Open worktree"
              disabled={!canOpenWorktree(worktree()) || isPending()}
              onSelect={() => open(worktree())}
            />
            <ContextMenuItem
              icon={FolderX}
              label="Remove worktree…"
              disabled={worktree().main || worktree().current || isPending()}
              onSelect={() => setRemoving(worktree())}
            />
          </>
        )}
      >
        {props.children({ ref: setSection, busy: isPending, open })}
      </RowMenu>
      <FailurePopover
        title={failure().title}
        error={failure().error}
        onDismiss={() => failure().dismiss()}
        anchor={section}
        placement="right-start"
      />
      <RemoveWorktreeDialog
        worktree={removing()}
        onCancel={() => setRemoving(undefined)}
        onRemove={(worktree) => {
          setRemoving(undefined);
          // Doesn't run while another worktree action is.
          remove.run(worktree.path);
        }}
      />
    </>
  );
}
