import { ContextMenu } from "@kobalte/core/context-menu";
import { useMenuContext } from "@kobalte/core/menu";
import GitBranchPlus from "lucide-solid/icons/git-branch-plus";
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

import { ContextMenuContent, ContextMenuItem } from "@/components/ui/context-menu";
import { useSwitchBranchState } from "@/git/queries/branch";

import { CreateBranchDialog } from "./create-branch-dialog";

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
 * The element marked with the branch `event` is on, if any. On a list that has the focus, as when
 * a key opens the menu, the first one in its active option: the history keeps the focus on its
 * list rather than on a row.
 */
function branchElement(event: Event): HTMLElement | null {
  if (!(event.target instanceof Element)) return null;
  const marked = event.target.closest<HTMLElement>("[data-branch]");
  if (marked) return marked;
  const active = event.target.getAttribute("aria-activedescendant");
  const option = active ? document.getElementById(active) : null;
  return option?.querySelector<HTMLElement>("[data-branch]") ?? null;
}

/**
 * Right-clicks `element`, below its start: a menu opened with a key on a list, again from the
 * branch in its active option, to open by that rather than where the list is.
 */
function reopenAt(element: HTMLElement) {
  const box = element.getBoundingClientRect();
  element.dispatchEvent(
    new MouseEvent("contextmenu", {
      bubbles: true,
      cancelable: true,
      clientX: box.left,
      clientY: box.bottom,
    }),
  );
}

/**
 * The menu of what can be done with a branch, opened by right-clicking an element in `children`
 * marked with its full ref name as `data-branch`, like a row in the sidebar or a label in the
 * history. Right-clicking anything else opens nothing. One menu for every element, rather than one
 * each: there can be thousands, made and dropped as the lists scroll. Elements rendered elsewhere
 * through a portal count too, as their events bubble through `children`. Needs a
 * `BranchMenuProvider` around it.
 */
export function BranchMenu(props: { repositoryId: string; children: JSX.Element }) {
  const createFrom = useContext(CreateFrom);
  if (!createFrom) throw new Error("A BranchMenu needs a BranchMenuProvider around it");
  const switching = useSwitchBranchState(() => props.repositoryId);
  const [open, setOpen] = createSignal(false);
  // The branch it's open for, by its full ref name, and what takes the focus back once it closes,
  // kept as they were when it opened.
  const [target, setTarget] = createSignal<{ branch: string; focus: HTMLElement | undefined }>();

  /** Takes the branch whose element `event` is on as the target; that element, if any. */
  const aim = (event: Event) => {
    const element = branchElement(event);
    // What has the focus: the row right-clicked, or the history's list, which keeps it rather than
    // its rows. Else the row, as at a long press, which focuses it only after this.
    const focused = document.activeElement;
    const focus =
      focused instanceof HTMLElement && focused !== document.body
        ? focused
        : (element?.closest("button") ?? undefined);
    setTarget(element ? { branch: element.dataset.branch!, focus } : undefined);
    return element;
  };

  return (
    <OpenFor.Provider value={() => (open() ? target()?.branch : undefined)}>
      <ContextMenu onOpenChange={setOpen}>
        <ContextMenu.Trigger
          as="div"
          // Laid out as if it weren't there, so what it holds is laid out as before.
          class="contents"
          // Read by Kobalte after the handlers below, which aim it: nothing opens off a branch, nor
          // at a long press there.
          disabled={!target()}
          // Kobalte opens it where it was right-clicked, unless this is prevented.
          onContextMenu={(event) => {
            const element = aim(event);
            if (element?.contains(event.target as Node)) return;
            event.preventDefault();
            if (element) reopenAt(element);
          }}
          // Kobalte opens it at a long press of a finger or pen.
          onPointerDown={(event) => event.pointerType !== "mouse" && aim(event)}
        >
          {props.children}
        </ContextMenu.Trigger>
        <ContextMenuContent
          // Back to what had the focus, rather than to this, which can't take focus.
          onCloseAutoFocus={(event) => {
            const focus = target()?.focus;
            if (!focus?.isConnected) return;
            event.preventDefault();
            focus.focus({ preventScroll: true });
          }}
        >
          <CloseOnChange value={props.repositoryId} />
          <Show when={target()}>
            {(current) => (
              <ContextMenuItem
                icon={GitBranchPlus}
                label="Create branch…"
                disabled={switching.isPending()}
                onSelect={() => createFrom(current().branch)}
              />
            )}
          </Show>
        </ContextMenuContent>
      </ContextMenu>
    </OpenFor.Provider>
  );
}

/**
 * Closes the menu it's in once `value` changes, like the repository: another repository's branch
 * isn't one of this one's.
 */
function CloseOnChange(props: { value: string }) {
  const menu = useMenuContext();
  createEffect(
    on(
      () => props.value,
      () => menu.close(),
      { defer: true },
    ),
  );
  return null;
}
