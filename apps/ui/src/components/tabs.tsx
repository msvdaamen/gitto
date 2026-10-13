import { Link, useLocation, useNavigate, useParams } from "@tanstack/solid-router";
import { cn } from "cn";
import GitBranch from "lucide-solid/icons/git-branch";
import Plus from "lucide-solid/icons/plus";
import X from "lucide-solid/icons/x";
import {
  batch,
  createEffect,
  createSignal,
  For,
  onCleanup,
  Show,
  splitProps,
  Suspense,
  type JSX,
} from "solid-js";

import {
  type Repository,
  useOpenRepository,
  useRemoveRepository,
  useReorderRepositories,
  useRepositories,
} from "@/hooks/repositories";

import { RemoveRepositoryDialog } from "./remove-repository-dialog";
import { IconButton } from "./ui/button";
import { FailurePopover } from "./ui/failure-popover";
import { GittoIcon } from "./ui/gitto-icon";

const HOME_TAB = "home";

/** How far a tab is moved before it's dragged rather than clicked, in pixels. */
const DRAG_THRESHOLD = 4;

/** The ids in `list` that are in `other` too, in `list`'s order. */
const common = (list: string[], other: string[]) => list.filter((id) => other.includes(id)).join();

/** Whether the ids in both `a` and `b` are in the same order in each. */
const sameOrder = (a: string[], b: string[]) => common(a, b) === common(b, a);

/** Keeps the grabbing cursor and stops text selection anywhere on the page while dragging. */
function setPageDragging(on: boolean) {
  document.documentElement.style.cursor = on ? "grabbing" : "";
  document.documentElement.style.userSelect = on ? "none" : "";
}

/**
 * A tab per repository, after the home tab. The open route decides which one is selected: none, on
 * a page of its own like the settings. The repositories' tabs are dragged, or moved with Alt+Shift
 * and the arrow keys, into another order.
 */
export function Tabs() {
  const params = useParams({ strict: false });
  const location = useLocation();
  const navigate = useNavigate();
  const repositories = useRepositories();
  const openRepository = useOpenRepository();
  const removeRepository = useRemoveRepository();
  const reorderRepositories = useReorderRepositories();
  const selectedTab = () => params().repoId ?? (location().pathname === "/" ? HOME_TAB : undefined);

  const savedOrder = () => repositories.data?.map((repository) => repository.id) ?? [];
  /** The tabs' order while one is dragged, and once it's dropped until that's saved. */
  const [movedOrder, setMovedOrder] = createSignal<string[]>();
  /**
   * The repositories' ids in the order their tabs are shown. Ids rather than the repositories, so
   * the tabs stay as they are when the list is read again, as new objects.
   */
  const ordered = () => movedOrder() ?? savedOrder();
  /** The tab being dragged, and how far it's been dragged from its place in the order. */
  const [dragged, setDragged] = createSignal<{ id: string; shift: number }>();

  // The tabs show the saved order again once the order they were moved into is read back.
  createEffect(() => {
    const order = movedOrder();
    if (order && !dragged() && sameOrder(order, savedOrder())) {
      setMovedOrder(undefined);
    }
  });

  // Measure the selected tab so the underline can slide to it.
  const tabElements = new Map<string, HTMLElement>();
  const [underline, setUnderline] = createSignal<{ left: number; width: number }>();

  function updateUnderline() {
    const selected = selectedTab();
    const el = selected && tabElements.get(selected);
    // Tabs move in the order while one is dragged, and the dragged one with the pointer.
    movedOrder();
    const dragging = dragged();
    const shift = dragging && dragging.id === selected ? dragging.shift : 0;
    setUnderline(el ? { left: el.offsetLeft + shift, width: el.offsetWidth } : undefined);
  }

  // Tabs resize at breakpoints, so remeasure whenever any of them changes size.
  const resizeObserver = new ResizeObserver(updateUnderline);
  onCleanup(() => resizeObserver.disconnect());

  function registerTab(id: string, el: HTMLElement) {
    tabElements.set(id, el);
    resizeObserver.observe(el);
    onCleanup(() => {
      resizeObserver.unobserve(el);
      tabElements.delete(id);
    });
  }

  createEffect(updateUnderline);

  /** The repository whose tab's close button was clicked, asked about before it's removed. */
  const [removing, setRemoving] = createSignal<Repository>();

  function remove(id: string) {
    removeRepository.mutate(id, {
      onSuccess: () => {
        if (selectedTab() === id) void navigate({ to: "/" });
      },
    });
  }
  /** Why the repository `id` couldn't be removed, if it's the last one that was tried. */
  const removeError = (id: string) =>
    removeRepository.variables === id ? removeRepository.error : null;

  /** The tab last moved, which says why if the new order couldn't be saved. */
  const [moved, setMoved] = createSignal<string>();
  const moveError = (id: string) => (moved() === id ? reorderRepositories.error : null);

  /** Shows the tabs in `order` (with `id` moved) until it's saved, unless it's the order they had. */
  function reorder(id: string, order: string[]) {
    setMovedOrder(order);
    if (sameOrder(order, savedOrder())) return;
    setMoved(id);
    reorderRepositories.mutate(order, { onError: () => setMovedOrder(undefined) });
  }

  /** Moves the tab `id` one place left (`-1`) or right (`1`), keeping it focused. */
  function move(id: string, by: -1 | 1) {
    const order = [...ordered()];
    const from = order.indexOf(id);
    const to = from + by;
    if (to < 0 || to >= order.length) return;
    order.splice(to, 0, ...order.splice(from, 1));
    reorder(id, order);
    // Moving the tab's element took the focus off it.
    tabElements.get(id)?.querySelector("a")?.focus();
  }

  /** The tab pressed, until the pointer's let go of: dragged once it's moved far enough. */
  let pressed: { id: string; pointerId: number; x: number; grabOffset: number } | undefined;
  /** Whether a tab was just dropped, so the click that may follow it doesn't open it. */
  let dropped = false;
  let container: HTMLDivElement | undefined;
  /** The element of the repository `id`'s tab. */
  const tab = (id: string) => tabElements.get(id)!;

  function startPress(id: string, event: PointerEvent & { currentTarget: HTMLElement }) {
    // The close button is clicked, not dragged.
    if (event.button !== 0 || (event.target as Element).closest("button")) return;
    pressed = {
      id,
      pointerId: event.pointerId,
      x: event.clientX,
      grabOffset: event.clientX - event.currentTarget.getBoundingClientRect().left,
    };
  }

  function drag(event: PointerEvent & { currentTarget: HTMLElement }) {
    const press = pressed;
    if (!press || event.pointerId !== press.pointerId) return;
    if (!dragged()) {
      if (Math.abs(event.clientX - press.x) < DRAG_THRESHOLD) return;
      event.currentTarget.setPointerCapture(event.pointerId);
      setPageDragging(true);
      batch(() => {
        setMovedOrder(ordered());
        setDragged({ id: press.id, shift: 0 });
      });
    }
    const order = ordered();
    const width = event.currentTarget.offsetWidth;

    // Where its left edge is with the pointer, kept between the first tab's left and the last's
    // right, in the tabs' coordinates as `offsetLeft` is.
    const first = tab(order[0]!);
    const last = tab(order.at(-1)!);
    const left = Math.min(
      Math.max(
        event.clientX - container!.getBoundingClientRect().left - press.grabOffset,
        first.offsetLeft,
      ),
      last.offsetLeft + last.offsetWidth - width,
    );
    // It goes after every other tab whose middle it's passed.
    const others = order.filter((id) => id !== press.id);
    const index = others.filter((id) => {
      const el = tab(id);
      return el.offsetLeft + el.offsetWidth / 2 < left + width / 2;
    }).length;
    if (order.indexOf(press.id) !== index) {
      others.splice(index, 0, press.id);
      setMovedOrder(others);
    }
    setDragged({ id: press.id, shift: left - event.currentTarget.offsetLeft });
  }

  function drop(event: PointerEvent & { currentTarget: HTMLElement }, cancelled: boolean) {
    const press = pressed;
    if (!press || event.pointerId !== press.pointerId) return;
    pressed = undefined;
    if (!dragged()) return;
    event.currentTarget.releasePointerCapture(event.pointerId);
    setPageDragging(false);
    setDragged(undefined);
    // The click comes straight after the pointer's let go of, if there's one.
    dropped = true;
    setTimeout(() => (dropped = false));
    if (cancelled) setMovedOrder(undefined);
    else reorder(press.id, ordered());
  }

  onCleanup(() => dragged() && setPageDragging(false));

  return (
    <div ref={(el) => (container = el)} class="relative flex">
      <HomeTab ref={(el) => registerTab(HOME_TAB, el)} active={selectedTab() === HOME_TAB} />
      {/* The home tab shows straight away; the repositories' tabs once they're loaded. */}
      <Suspense>
        <For each={ordered()}>
          {(id) => (
            <Show when={repositories.data?.find((repository) => repository.id === id)}>
              {(repository) => (
                <FailurePopover
                  title={`${moveError(id) ? "Move" : "Remove"} ${repository().name}`}
                  error={moveError(id) ?? removeError(id)}
                  onDismiss={() => {
                    reorderRepositories.reset();
                    removeRepository.reset();
                  }}
                >
                  <RepositoryTab
                    ref={(el) => registerTab(id, el)}
                    id={id}
                    name={repository().name}
                    active={selectedTab() === id}
                    shift={dragged()?.id === id ? dragged()!.shift : undefined}
                    onRemove={() => setRemoving(repository())}
                    onMove={(by) => move(id, by)}
                    onPointerDown={(event) => startPress(id, event)}
                    onPointerMove={drag}
                    onPointerUp={(event) => drop(event, false)}
                    onPointerCancel={(event) => drop(event, true)}
                    onClick={(event) => dropped && event.preventDefault()}
                  />
                </FailurePopover>
              )}
            </Show>
          )}
        </For>
      </Suspense>
      <FailurePopover
        title="Open repository"
        error={openRepository.error()}
        onDismiss={openRepository.dismiss}
        class="ml-1.25 shrink-0 self-center"
      >
        <IconButton label="Open new repository" icon={Plus} onClick={openRepository.open} />
      </FailurePopover>

      {/* Only mounted once measured, so it doesn't slide in from the left on first render. */}
      <Show when={underline()}>
        {(pos) => (
          <div
            class={cn(
              "absolute -bottom-px h-0.5 bg-primary transition-all duration-200 ease-out motion-reduce:transition-none",
              // It follows a dragged tab as it is, rather than catching up.
              dragged() && "transition-none",
            )}
            style={{ left: `${pos().left}px`, width: `${pos().width}px` }}
          />
        )}
      </Show>

      <RemoveRepositoryDialog
        repository={removing()}
        onCancel={() => setRemoving(undefined)}
        onRemove={(repository) => {
          setRemoving(undefined);
          remove(repository.id);
        }}
      />
    </div>
  );
}

type PointerHandler = (event: PointerEvent & { currentTarget: HTMLDivElement }) => void;

function RepositoryTab(props: {
  ref: (el: HTMLDivElement) => void;
  id: string;
  name: string;
  active: boolean;
  /** How far it's been dragged from its place, while it's being dragged. */
  shift: number | undefined;
  onRemove: () => void;
  /** Moves it one place left (`-1`) or right (`1`). */
  onMove: (by: -1 | 1) => void;
  onPointerDown: PointerHandler;
  onPointerMove: PointerHandler;
  onPointerUp: PointerHandler;
  onPointerCancel: PointerHandler;
  /** On a click on its link, which `preventDefault` keeps from opening it. */
  onClick: (event: MouseEvent) => void;
}) {
  return (
    <TabFrame
      ref={props.ref}
      active={props.active}
      class={cn(
        "group min-w-30 max-w-45 border-r border-border-soft text-muted hover:text-text-soft max-sm:min-w-26.25",
        props.shift !== undefined && "relative z-10 bg-bg shadow-app",
      )}
      style={props.shift !== undefined ? { transform: `translateX(${props.shift}px)` } : undefined}
      onPointerDown={props.onPointerDown}
      onPointerMove={props.onPointerMove}
      onPointerUp={props.onPointerUp}
      onPointerCancel={props.onPointerCancel}
    >
      <Link
        to="/$repoId"
        params={{ repoId: props.id }}
        // Dragged as a tab, not as a link to drop elsewhere.
        draggable={false}
        onClick={(event) => props.onClick(event)}
        aria-keyshortcuts="Alt+Shift+ArrowLeft Alt+Shift+ArrowRight"
        onKeyDown={(event) => {
          if (!event.altKey || !event.shiftKey) return;
          const by = event.key === "ArrowLeft" ? -1 : event.key === "ArrowRight" ? 1 : undefined;
          if (!by) return;
          event.preventDefault();
          props.onMove(by);
        }}
        class="flex h-full min-w-0 flex-1 items-center gap-2 pl-2.5 focus-ring"
      >
        <span class="grid size-5.5 shrink-0 place-items-center rounded-md bg-blue-soft text-blue">
          <GitBranch size={14} />
        </span>
        <span class={cn("flex-1 truncate text-left font-[590]", props.active && "text-text")}>
          {props.name}
        </span>
      </Link>
      <button
        class={cn(
          "mr-2.5 ml-2 grid size-5 shrink-0 cursor-pointer place-items-center rounded-md border-0 bg-transparent p-0 text-muted opacity-0 group-hover:opacity-100 hover:bg-panel-hover hover:text-text focus-visible:opacity-100 focus-ring",
          props.active && "opacity-100",
        )}
        aria-label={`Remove ${props.name}`}
        title={`Remove ${props.name}`}
        onClick={() => props.onRemove()}
      >
        <X size={13} />
      </button>
    </TabFrame>
  );
}

function HomeTab(props: { ref: (el: HTMLDivElement) => void; active: boolean }) {
  return (
    <TabFrame
      ref={props.ref}
      active={props.active}
      class="w-29 shrink-0 basis-29 max-md:w-13.25 max-md:basis-13.25"
    >
      <Link
        to="/"
        class="flex h-full flex-1 items-center gap-2.25 px-3.5 focus-ring max-md:justify-center max-md:px-0"
        aria-label="Home"
      >
        <GittoIcon class="h-5.75 w-6.75 shrink-0" />
        <span class="text-[17px] font-[750] tracking-[-.5px] max-md:hidden">gitto</span>
      </Link>
    </TabFrame>
  );
}

/** A tab's box: highlighted while selected, measured by `Tabs` to place the underline. */
function TabFrame(
  props: JSX.HTMLAttributes<HTMLDivElement> & {
    ref: (el: HTMLDivElement) => void;
    active: boolean;
    class: string;
  },
) {
  const [local, rest] = splitProps(props, ["active", "class"]);
  return (
    <div
      {...rest}
      class={cn(
        "flex h-12 items-center",
        local.active ? "bg-bg" : "hover:bg-panel-hover",
        local.class,
      )}
    />
  );
}
