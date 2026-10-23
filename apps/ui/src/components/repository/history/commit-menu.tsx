import PenLine from "lucide-solid/icons/pen-line";
import { createContext, createSignal, useContext, type Accessor, type JSX } from "solid-js";

import { ContextMenuItem } from "@/components/ui/context-menu";
import { useOperationInProgress } from "@/git/queries/progress";
import { useStatus } from "@/git/queries/status";
import { useUnsuspendedData } from "@/git/queries/unsuspended";
import { useResetOn } from "@/hooks/reset-on";

import { RowMenu } from "../row-menu";
import { RewordDialog } from "./reword-dialog";

/** The SHA of the commit whose menu is open, if any. */
const OpenFor = createContext<Accessor<string | undefined>>(() => undefined);

/**
 * The SHA of the commit whose menu is open, if any, for its row to show which one it's for (see
 * `CommitMenu`).
 */
export function useCommitMenuOpenFor() {
  return useContext(OpenFor);
}

/**
 * The menu of what can be done with a commit, opened by right-clicking its row in the history
 * `children` renders: one marked with its SHA as `data-commit` (see `RowMenu`). Only the last
 * commit of the checked-out branch has one, to edit its message (see `RewordDialog`); not while an
 * operation (a merge, a rebase…) is under way, as git won't amend then. A branch's label in the row
 * opens the branch's menu instead, from a `BranchMenu` inside this one.
 */
export function CommitMenu(props: {
  repositoryId: string;
  /** Told a commit was reworded, by its SHA before and after, e.g. to keep it selected. */
  onReworded: (from: string, to: string) => void;
  children: JSX.Element;
}) {
  const status = useUnsuspendedData(useStatus(() => props.repositoryId));
  const operation = useUnsuspendedData(useOperationInProgress(() => props.repositoryId));
  const [openFor, setOpenFor] = createSignal<string>();
  // The commit whose message is being edited.
  const [rewording, setRewording] = createSignal<string>();
  // A commit of one repository isn't one of another.
  useResetOn(
    () => props.repositoryId,
    () => setRewording(undefined),
  );

  /** The last commit of the checked-out branch; none with HEAD detached, or before the first. */
  const branchTip = () => {
    const head = status()?.head;
    return head?.kind === "branch" ? head.sha : undefined;
  };

  return (
    <OpenFor.Provider value={openFor}>
      <RowMenu
        repositoryId={props.repositoryId}
        attribute="commit"
        item={(sha) => (sha === branchTip() ? sha : undefined)}
        onOpenFor={setOpenFor}
        actions={(sha) => (
          <ContextMenuItem
            icon={PenLine}
            label="Edit message…"
            // Also until it's known whether one is.
            disabled={operation() !== null}
            onSelect={() => setRewording(sha())}
          />
        )}
      >
        {props.children}
      </RowMenu>
      <RewordDialog
        repositoryId={props.repositoryId}
        sha={rewording()}
        onClose={() => setRewording(undefined)}
        onReworded={props.onReworded}
      />
    </OpenFor.Provider>
  );
}
