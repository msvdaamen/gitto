import type { Stash } from "@gitto/git/types";
import ArchiveRestore from "lucide-solid/icons/archive-restore";
import Trash from "lucide-solid/icons/trash";
import { createEffect, createSignal, on, type JSX } from "solid-js";

import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ContextMenuItem } from "@/components/ui/context-menu";
import { FailurePopover } from "@/components/ui/failure-popover";
import { useStashActions, useStashes } from "@/git/queries/stash";
import { useStatus } from "@/git/queries/status";
import { canPop } from "@/git/stash";

import { RowMenu } from "../row-menu";

/** What the stashes' section is given by their menu (see `StashMenu`). */
export interface StashSectionFeedback {
  /** Takes the section's element, which why an action failed is shown beside. */
  ref: (element: HTMLElement) => void;
  /** Whether a stash is being popped or deleted. */
  busy: () => boolean;
}

/**
 * The menu of what can be done with a stash, opened by right-clicking its row in the section
 * `children` renders: one marked with its SHA as `data-stash` (see `RowMenu`). Popping it puts its
 * changes back and drops it; deleting it asks first, as its changes are lost. Like the toolbar's,
 * one at a time. The section shows each running, and why it failed beside it.
 */
export function StashMenu(props: {
  repositoryId: string;
  /** Told which stash's menu is open, by its SHA, if any, to show which one it's for. */
  onOpenFor: (sha: string | undefined) => void;
  children: (section: StashSectionFeedback) => JSX.Element;
}) {
  const status = useStatus(() => props.repositoryId);
  const stashes = useStashes(() => props.repositoryId);
  const { popPicked, drop, isPending } = useStashActions(() => props.repositoryId);
  // The stash being asked about deleting, and the stashes' section.
  const [dropping, setDropping] = createSignal<Stash>();
  const [section, setSection] = createSignal<HTMLElement>();
  // A stash of one repository isn't one of another.
  createEffect(
    on(
      () => props.repositoryId,
      () => setDropping(undefined),
      { defer: true },
    ),
  );

  // One at a time, the deletion's first: both can have failed, one after the other.
  const failure = () =>
    drop.error()
      ? { title: "Delete stash", error: drop.error(), dismiss: () => drop.dismiss() }
      : { title: "Pop stash", error: popPicked.error(), dismiss: () => popPicked.dismiss() };

  return (
    <>
      <RowMenu
        repositoryId={props.repositoryId}
        attribute="stash"
        item={(sha) => stashes.data?.find((stash) => stash.sha === sha)}
        onOpenFor={(sha) => props.onOpenFor(sha)}
        actions={(stash) => (
          <>
            <ContextMenuItem
              icon={ArchiveRestore}
              label="Pop stash"
              disabled={!canPop(status.data, stashes.data) || isPending()}
              onSelect={() => popPicked.run(stash().sha)}
            />
            <ContextMenuItem
              icon={Trash}
              label="Delete stash…"
              disabled={isPending()}
              onSelect={() => setDropping(stash())}
            />
          </>
        )}
      >
        {props.children({
          ref: setSection,
          busy: () => popPicked.isPending() || drop.isPending(),
        })}
      </RowMenu>
      <FailurePopover
        title={failure().title}
        error={failure().error}
        onDismiss={() => failure().dismiss()}
        anchor={section}
        placement="right-start"
      />
      <ConfirmDialog
        item={dropping()}
        icon={Trash}
        title={() => "Delete stash?"}
        description={(stash) => `"${stash.message}" is deleted, and the changes in it are lost.`}
        confirmLabel="Delete"
        onCancel={() => setDropping(undefined)}
        onConfirm={(stash) => {
          setDropping(undefined);
          // Doesn't run while another stash action is.
          drop.run(stash.sha);
        }}
      />
    </>
  );
}
