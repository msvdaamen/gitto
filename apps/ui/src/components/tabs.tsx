import { Link, useNavigate, useParams } from "@tanstack/solid-router";
import { cn } from "cn";
import GitBranch from "lucide-solid/icons/git-branch";
import Plus from "lucide-solid/icons/plus";
import X from "lucide-solid/icons/x";
import { createEffect, createSignal, For, onCleanup, Show, Suspense, type JSX } from "solid-js";

import { useOpenRepository, useRemoveRepository, useRepositories } from "@/hooks/repositories";

import { FailurePopover } from "./repository/failure-popover";
import { IconButton } from "./ui/button";
import { GittoIcon } from "./ui/gitto-icon";

const HOME_TAB = "home";

/** A tab per repository, after the home tab. The open route decides which one is selected. */
export function Tabs() {
  const params = useParams({ strict: false });
  const navigate = useNavigate();
  const repositories = useRepositories();
  const openRepository = useOpenRepository();
  const removeRepository = useRemoveRepository();
  const selectedTab = () => params().repoId ?? HOME_TAB;

  // Measure the selected tab so the underline can slide to it.
  const tabElements = new Map<string, HTMLElement>();
  const [underline, setUnderline] = createSignal<{ left: number; width: number }>();

  function updateUnderline() {
    const el = tabElements.get(selectedTab());
    setUnderline(el ? { left: el.offsetLeft, width: el.offsetWidth } : undefined);
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

  return (
    <div class="relative flex">
      <HomeTab ref={(el) => registerTab(HOME_TAB, el)} active={selectedTab() === HOME_TAB} />
      {/* The home tab shows straight away; the repositories' tabs once they're loaded. */}
      <Suspense>
        <For each={repositories.data}>
          {(repository) => (
            <FailurePopover
              title={`Remove ${repository.name}`}
              error={removeError(repository.id)}
              onDismiss={() => removeRepository.reset()}
            >
              <RepositoryTab
                ref={(el) => registerTab(repository.id, el)}
                id={repository.id}
                name={repository.name}
                active={selectedTab() === repository.id}
                onRemove={() => remove(repository.id)}
              />
            </FailurePopover>
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
            class="absolute -bottom-px h-0.5 bg-primary transition-all duration-200 ease-out motion-reduce:transition-none"
            style={{ left: `${pos().left}px`, width: `${pos().width}px` }}
          />
        )}
      </Show>
    </div>
  );
}

function RepositoryTab(props: {
  ref: (el: HTMLDivElement) => void;
  id: string;
  name: string;
  active: boolean;
  onRemove: () => void;
}) {
  return (
    <TabFrame
      ref={props.ref}
      active={props.active}
      class="group min-w-30 max-w-45 border-r border-border-soft text-muted hover:text-text-soft max-sm:min-w-26.25"
    >
      <Link
        to="/$repoId"
        params={{ repoId: props.id }}
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
function TabFrame(props: {
  ref: (el: HTMLDivElement) => void;
  active: boolean;
  class: string;
  children: JSX.Element;
}) {
  return (
    <div
      ref={props.ref}
      class={cn(
        "flex h-12 items-center",
        props.active ? "bg-bg" : "hover:bg-panel-hover",
        props.class,
      )}
    >
      {props.children}
    </div>
  );
}
