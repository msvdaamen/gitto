import GitBranchPlus from "lucide-solid/icons/git-branch-plus";
import {
  createContext,
  createEffect,
  createSignal,
  on,
  useContext,
  type Accessor,
  type JSX,
} from "solid-js";

import { ContextMenuItem } from "@/components/ui/context-menu";
import { useSwitchBranchState } from "@/git/queries/branch";

import { CreateBranchDialog } from "./create-branch-dialog";
import { RowMenu } from "./row-menu";

/** Begins creating a branch from another, by its full ref name, asking for the new one's name. */
const CreateFrom = createContext<(from: string) => void>();

/** The full ref name of the branch whose menu is open, if any. */
const OpenFor = createContext<Accessor<string | undefined>>(() => undefined);

/**
 * What the branch menus in `children` (see `BranchMenu`) share: the dialog naming a branch to
 * create, so a name kept to fix after a failure is there whichever menu it's opened from again.
 */
export function BranchMenuProvider(props: { repositoryId: string; children: JSX.Element }) {
  // The branch a new one is being named to be created from, by its full ref name.
  const [from, setFrom] = createSignal<string>();
  createEffect(
    on(
      () => props.repositoryId,
      () => setFrom(undefined),
      { defer: true },
    ),
  );

  return (
    <CreateFrom.Provider value={setFrom}>
      {props.children}
      <CreateBranchDialog
        repositoryId={props.repositoryId}
        from={from()}
        onClose={() => setFrom(undefined)}
      />
    </CreateFrom.Provider>
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
 * The menu of what can be done with a branch, opened by right-clicking an element in `children`
 * marked with its full ref name as `data-branch`, like a row in the sidebar or a label in the
 * history (see `RowMenu`). Needs a `BranchMenuProvider` around it.
 */
export function BranchMenu(props: { repositoryId: string; children: JSX.Element }) {
  const createFrom = useContext(CreateFrom);
  if (!createFrom) throw new Error("A BranchMenu needs a BranchMenuProvider around it");
  const switching = useSwitchBranchState(() => props.repositoryId);
  const [openFor, setOpenFor] = createSignal<string>();

  return (
    <OpenFor.Provider value={openFor}>
      <RowMenu
        repositoryId={props.repositoryId}
        attribute="branch"
        item={(fullName) => fullName}
        onOpenFor={setOpenFor}
        actions={(branch) => (
          <ContextMenuItem
            icon={GitBranchPlus}
            label="Create branch…"
            disabled={switching.isPending()}
            onSelect={() => createFrom(branch())}
          />
        )}
      >
        {props.children}
      </RowMenu>
    </OpenFor.Provider>
  );
}
