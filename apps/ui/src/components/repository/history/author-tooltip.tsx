import type { JSX } from "@solidjs/web";
import { Tooltip, TooltipArrow, TooltipContent } from "@ui-sdk/solid/tooltip";
import { createContext, createEffect, createSignal, onCleanup, Show, useContext } from "solid-js";

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

const AuthorTooltipContext = createContext<AuthorTooltip | null>(null);

/** The tooltip the graph's nodes show their author in; `null` outside an `AuthorTooltipProvider`. */
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

  createEffect(
    () => !!shown(),
    (isShown) => {
      if (!isShown) return;
      const onKeyDown = (event: KeyboardEvent) => event.key === "Escape" && close();
      // Scrolling moves the node away from under the pointer, without the pointer leaving it.
      window.addEventListener("scroll", close, { capture: true, passive: true });
      window.addEventListener("pointerdown", close, { capture: true });
      window.addEventListener("keydown", onKeyDown);
      return () => {
        window.removeEventListener("scroll", close, { capture: true });
        window.removeEventListener("pointerdown", close, { capture: true });
        window.removeEventListener("keydown", onKeyDown);
      };
    },
  );

  return (
    <AuthorTooltipContext value={tooltip}>
      {props.children}
      {/* Opened and closed by the nodes, rather than by a trigger of its own. */}
      <Tooltip open={!!shown()} onOpenChange={(open) => !open && close()}>
        <TooltipContent
          // An SVG element, which is positioned against like any other.
          anchor={shown()?.node ?? null}
          side="top"
          sideOffset={6}
          class="pointer-events-none z-50 flex max-w-[280px] animate-tooltip-in items-center gap-2 rounded-md border border-border bg-panel-hover py-1 pr-2.5 pl-1 text-[12px] text-text shadow-app motion-reduce:animate-none"
        >
          <TooltipArrow class="absolute left-[calc(var(--arrow-offset)-5px)] size-2.5 rotate-45 border-border bg-panel-hover data-[side=bottom]:-top-[6px] data-[side=bottom]:border-t data-[side=bottom]:border-l data-[side=top]:-bottom-[6px] data-[side=top]:border-r data-[side=top]:border-b" />
          <Show when={shown()}>
            {(current) => (
              <>
                <Avatar initials={current().author.initials} color={current().author.color} />
                <span class="truncate font-[600]">{current().author.name}</span>
              </>
            )}
          </Show>
        </TooltipContent>
      </Tooltip>
    </AuthorTooltipContext>
  );
}
