import { Popper } from "@kobalte/core/popper";
import { createContext, createEffect, createSignal, onCleanup, Show, useContext } from "solid-js";
import type { JSX } from "solid-js";
import { Portal } from "solid-js/web";

import { Avatar } from "@/components/ui/avatar";

/** How long a node is hovered before its tooltip shows, so moving across the graph doesn't flash it. */
const OPEN_DELAY_MS = 150;

/** Who a commit is by, as its tooltip shows them. */
export interface GraphAuthor {
  name: string;
  initials: string;
  color?: string;
}

interface AuthorTooltip {
  /** Shows `author` above `node`, once it's been hovered for a moment. */
  show: (node: Element, author: GraphAuthor) => void;
  /** Hides `node`'s tooltip, or stops it from showing; not another node's, hovered since. */
  hide: (node: Element) => void;
}

const AuthorTooltipContext = createContext<AuthorTooltip>();

/** The tooltip the graph's nodes show their author in; `undefined` outside an `AuthorTooltipProvider`. */
export function useAuthorTooltip() {
  return useContext(AuthorTooltipContext);
}

/**
 * Shows the author of the commit whose node in the graph is hovered, in one tooltip for all the
 * rows in `children`. A tooltip of its own per row is set up again for every row that scrolls into
 * view, though at most one is ever shown.
 */
export function AuthorTooltipProvider(props: { children: JSX.Element }) {
  const [shown, setShown] = createSignal<{ node: Element; author: GraphAuthor }>();
  const [content, setContent] = createSignal<HTMLElement>();
  // The node whose tooltip is shown, or about to be.
  let hovered: Element | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const close = () => {
    clearTimeout(timer);
    hovered = undefined;
    setShown(undefined);
  };
  const tooltip: AuthorTooltip = {
    show(node, author) {
      clearTimeout(timer);
      hovered = node;
      timer = setTimeout(() => setShown({ node, author }), OPEN_DELAY_MS);
    },
    hide(node) {
      if (node === hovered) close();
    },
  };
  onCleanup(() => clearTimeout(timer));

  createEffect(() => {
    if (!shown()) return;
    const onKeyDown = (event: KeyboardEvent) => event.key === "Escape" && close();
    // Scrolling moves the node away from under the pointer, without the pointer leaving it.
    window.addEventListener("scroll", close, { capture: true, passive: true });
    window.addEventListener("pointerdown", close, { capture: true });
    window.addEventListener("keydown", onKeyDown);
    onCleanup(() => {
      window.removeEventListener("scroll", close, { capture: true });
      window.removeEventListener("pointerdown", close, { capture: true });
      window.removeEventListener("keydown", onKeyDown);
    });
  });

  return (
    <AuthorTooltipContext.Provider value={tooltip}>
      {props.children}
      <Popper
        // An SVG element, which is positioned against like any other.
        anchorRef={() => shown()?.node as HTMLElement | undefined}
        contentRef={content}
        placement="top"
        gutter={6}
      >
        <Show when={shown()}>
          {(current) => (
            // At the end of the page, so scroll areas don't clip it.
            <Portal>
              <Popper.Positioner class="pointer-events-none z-50">
                <div
                  ref={setContent}
                  role="tooltip"
                  class="relative flex max-w-[280px] animate-tooltip-in items-center gap-2 rounded-md border border-border bg-panel-hover py-1 pr-2.5 pl-1 text-[12px] text-text shadow-app motion-reduce:animate-none"
                >
                  <Popper.Arrow size={12} />
                  <Avatar initials={current().author.initials} color={current().author.color} />
                  <span class="truncate font-[600]">{current().author.name}</span>
                </div>
              </Popper.Positioner>
            </Portal>
          )}
        </Show>
      </Popper>
    </AuthorTooltipContext.Provider>
  );
}
