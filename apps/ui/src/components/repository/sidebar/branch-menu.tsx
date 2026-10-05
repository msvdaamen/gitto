import type { Ref } from "@gitto/git/types";
import { ContextMenu } from "@kobalte/core/context-menu";
import GitBranchPlus from "lucide-solid/icons/git-branch-plus";
import { createSignal, Show, type JSX } from "solid-js";

import { ContextMenuContent, ContextMenuItem } from "@/components/ui/context-menu";

/**
 * The menu of what can be done with a branch, opened by right-clicking its row in `children`: one
 * marked with its full ref name as `data-branch`. Right-clicking anything else opens nothing. One
 * menu for every row, rather than one each: there can be thousands, made and dropped as the lists
 * scroll.
 */
export function BranchMenu(props: {
  /** The branch by its full ref name; `undefined` if it's gone. */
  branch: (fullName: string) => Ref | undefined;
  /** Whether a switch of branches is running: one at a time. */
  switching: boolean;
  /** Told which branch's menu is open, if any, to show which one it's for. */
  onOpenFor: (fullName: string | undefined) => void;
  onCreateBranch: (from: Ref) => void;
  children: JSX.Element;
}) {
  // The branch it's open for, and its row, kept as they were when it opened.
  const [target, setTarget] = createSignal<{ branch: Ref; row: HTMLElement }>();

  /** Takes the branch whose row `event` is on as the target; whether there is one. */
  const aim = (event: Event) => {
    const row =
      event.target instanceof Element ? event.target.closest<HTMLElement>("[data-branch]") : null;
    const branch = row ? props.branch(row.dataset.branch!) : undefined;
    setTarget(row && branch ? { branch, row } : undefined);
    return !!branch;
  };

  return (
    <ContextMenu
      onOpenChange={(open) => props.onOpenFor(open ? target()?.branch.fullName : undefined)}
    >
      <ContextMenu.Trigger
        as="div"
        // Laid out as if it weren't there, so the sections it holds share the sidebar as before.
        class="contents"
        // Kobalte opens it unless this is prevented.
        onContextMenu={(event) => !aim(event) && event.preventDefault()}
        // Kobalte opens it at a long press of a finger or pen.
        onPointerDown={(event) => event.pointerType !== "mouse" && aim(event)}
      >
        {props.children}
      </ContextMenu.Trigger>
      <ContextMenuContent
        // Back to the row, rather than to this, which can't take focus.
        onCloseAutoFocus={(event) => {
          const row = target()?.row;
          if (!row?.isConnected) return;
          event.preventDefault();
          row.focus();
        }}
      >
        <Show when={target()}>
          {(current) => (
            <ContextMenuItem
              icon={GitBranchPlus}
              label="Create branch…"
              disabled={props.switching}
              onSelect={() => props.onCreateBranch(current().branch)}
            />
          )}
        </Show>
      </ContextMenuContent>
    </ContextMenu>
  );
}
