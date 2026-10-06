import type { Ref } from "@gitto/git/types";
import GitBranchPlus from "lucide-solid/icons/git-branch-plus";
import type { JSX } from "solid-js";

import { ContextMenuItem } from "@/components/ui/context-menu";

import { RowMenu } from "./row-menu";

/**
 * The menu of what can be done with a branch, opened by right-clicking its row in `children`: one
 * marked with its full ref name as `data-branch` (see `RowMenu`).
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
  return (
    <RowMenu
      attribute="branch"
      item={(fullName) => props.branch(fullName)}
      onOpenFor={(fullName) => props.onOpenFor(fullName)}
      actions={(branch) => (
        <ContextMenuItem
          icon={GitBranchPlus}
          label="Create branch…"
          disabled={props.switching}
          onSelect={() => props.onCreateBranch(branch())}
        />
      )}
    >
      {props.children}
    </RowMenu>
  );
}
