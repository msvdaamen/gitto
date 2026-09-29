import { useParams, useRouter } from "@tanstack/solid-router";
import { cn } from "cn";
import GitBranch from "lucide-solid/icons/git-branch";
import Plus from "lucide-solid/icons/plus";
import { createEffect, createMemo, createSignal, For, onCleanup, Show } from "solid-js";

import { repositories } from "@/data/mock-data";

import { IconButton } from "./ui/button";
import { GittoIcon } from "./ui/gitto-icon";

const HOME_PAGE = "home-page";

type TabItem = {
  id: string;
  name: string;
};

export function Tabs() {
  const params = useParams({ strict: false });
  const router = useRouter();
  const [selectedTab, setSelectedTab] = createSignal<string>(params().repoId ?? HOME_PAGE);
  const tabs = createMemo<TabItem[]>(() => {
    return [
      { id: HOME_PAGE, name: "Home" },
      ...repositories.map((r) => ({ id: r.id, name: r.name })),
    ];
  });

  // Measure the active tab so the underline can slide to it.
  const tabRefs = new Map<string, HTMLElement>();
  const [underline, setUnderline] = createSignal<{ left: number; width: number }>();

  function updateUnderline() {
    const el = tabRefs.get(selectedTab());
    setUnderline(el ? { left: el.offsetLeft, width: el.offsetWidth } : undefined);
  }

  // Tabs resize at breakpoints, so remeasure whenever any of them changes size.
  const resizeObserver = new ResizeObserver(updateUnderline);
  onCleanup(() => resizeObserver.disconnect());

  function registerTab(id: string, el: HTMLElement) {
    tabRefs.set(id, el);
    resizeObserver.observe(el);
    onCleanup(() => {
      resizeObserver.unobserve(el);
      tabRefs.delete(id);
    });
  }

  createEffect(updateUnderline);

  function setTab(tab: TabItem) {
    setSelectedTab(tab.id);
    if (tab.id === HOME_PAGE) {
      router.navigate({
        to: `/`,
      });
      return;
    }
    router.navigate({
      to: `/$repoId`,
      params: { repoId: tab.id },
    });
  }

  return (
    <div class="flex">
      <div class="relative flex">
        <For each={tabs()}>
          {(tab) =>
            tab.id === HOME_PAGE ? (
              <TabHome
                ref={(el) => registerTab(tab.id, el)}
                isActive={selectedTab() === tab.id}
                onClick={() => setTab(tab)}
              />
            ) : (
              <Tab
                ref={(el) => registerTab(tab.id, el)}
                tab={tab}
                isActive={selectedTab() === tab.id}
                onClick={() => setTab(tab)}
              />
            )
          }
        </For>
        <IconButton label="Open new repository" icon={Plus} class="ml-1.25 shrink-0 self-center" />

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
    </div>
  );
}

function Tab(props: {
  ref: (el: HTMLDivElement) => void;
  tab: TabItem;
  isActive: boolean;
  onClick: () => void;
}) {
  return (
    <div
      ref={props.ref}
      class={cn(
        "flex h-12 min-w-30 max-w-45 cursor-pointer items-center gap-2 border-0 border-r border-border-soft bg-transparent px-2.5 text-muted hover:text-text-soft max-[700px]:min-w-26.25",
        props.isActive && "bg-bg text-text",
        !props.isActive && "hover:bg-panel-hover",
      )}
      onClick={props.onClick}
    >
      <span class="grid size-5.5 shrink-0 place-items-center rounded-md bg-blue-soft text-blue">
        <GitBranch size={14} />
      </span>
      <span class="flex-1 truncate text-left font-[590]">{props.tab.name}</span>
    </div>
  );
}

function TabHome(props: {
  ref: (el: HTMLDivElement) => void;
  isActive: boolean;
  onClick: () => void;
}) {
  return (
    <div
      ref={props.ref}
      class={cn(
        "flex w-29 shrink-0 basis-29 cursor-pointer items-center gap-2,25 border-0 bg-transparent px-3.5 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary max-[900px]:w-13.25 max-[900px]:basis-13.25 max-[900px]:justify-center max-[900px]:px-0",
        props.isActive && "bg-bg",
        !props.isActive && "hover:bg-panel-hover",
      )}
      onClick={props.onClick}
    >
      <GittoIcon class="h-5.75 w-6.75 shrink-0" />
      <span class="text-[17px] font-[750] tracking-[-.5px] max-[900px]:hidden">gitto</span>
    </div>
  );
}
