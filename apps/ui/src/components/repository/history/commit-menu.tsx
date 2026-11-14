import Copy from "lucide-solid/icons/copy";
import GitBranchPlus from "lucide-solid/icons/git-branch-plus";
import GitCommitHorizontal from "lucide-solid/icons/git-commit-horizontal";
import PenLine from "lucide-solid/icons/pen-line";
import { createContext, createSignal, Show, useContext, type Accessor, type JSX } from "solid-js";

import { ContextMenuItem } from "@/components/ui/context-menu";
import { useBranchesChanging, useCheckoutCommit } from "@/git/queries/branch";
import { useOperationInProgress } from "@/git/queries/progress";
import { useStatus } from "@/git/queries/status";
import { useUnsuspendedData } from "@/git/queries/unsuspended";
import { useResetOn } from "@/hooks/reset-on";

import { useCreateBranchAt } from "../branch-menu";
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
 * `children` renders: one marked with its SHA as `data-commit` (see `RowMenu`). Checking it out,
 * detaching HEAD at it, and creating a branch at it, asking for its name (in the dialog the branch
 * menus share, from the `BranchMenuProvider` around it), take the uncommitted changes along, or
 * keep them in the stash when they conflict with it; neither while another change of the branches
 * (a switch, a merge…) is running. The toolbar shows them running, and why they failed, as for a
 * switch of branches. The last commit of the checked-out branch can also have its message edited
 * (see `RewordDialog`); not while an operation (a merge, a rebase…) is under way, as git won't
 * amend then. Any commit's full SHA can be copied. A branch's label in the row opens the branch's
 * menu instead, from a `BranchMenu` inside this one.
 */
export function CommitMenu(props: {
  repositoryId: string;
  /** Told a commit was reworded, by its SHA before and after, e.g. to keep it selected. */
  onReworded: (from: string, to: string) => void;
  children: JSX.Element;
}) {
  const status = useUnsuspendedData(useStatus(() => props.repositoryId));
  const operation = useUnsuspendedData(useOperationInProgress(() => props.repositoryId));
  const busy = useBranchesChanging(() => props.repositoryId);
  // The toolbar shows it running, and why it failed.
  const checkout = useCheckoutCommit(() => props.repositoryId);
  const createBranchAt = useCreateBranchAt();
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
  /** The commit HEAD is detached at, if it is. */
  const detachedAt = () => {
    const head = status()?.head;
    return head?.kind === "detached" ? head.sha : undefined;
  };

  return (
    <OpenFor.Provider value={openFor}>
      <RowMenu
        repositoryId={props.repositoryId}
        attribute="commit"
        item={(sha) => sha}
        onOpenFor={setOpenFor}
        actions={(sha) => (
          <>
            <ContextMenuItem
              icon={GitCommitHorizontal}
              label="Check out this commit"
              disabled={busy() || sha() === detachedAt()}
              onSelect={() => checkout.run(sha())}
            />
            <ContextMenuItem
              icon={GitBranchPlus}
              label="Create branch here…"
              disabled={busy()}
              onSelect={() => createBranchAt(sha())}
            />
            <Show when={sha() === branchTip()}>
              <ContextMenuItem
                icon={PenLine}
                label="Edit message…"
                // Also until it's known whether one is.
                disabled={operation() !== null}
                onSelect={() => setRewording(sha())}
              />
            </Show>
            <ContextMenuItem
              icon={Copy}
              label="Copy SHA"
              onSelect={() =>
                void navigator.clipboard
                  .writeText(sha())
                  .catch((error: unknown) => console.error("Couldn't copy the SHA", error))
              }
            />
          </>
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
