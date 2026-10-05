import { cn } from "cn";
import ListTree from "lucide-solid/icons/list-tree";
import LoaderCircle from "lucide-solid/icons/loader-circle";
import Settings from "lucide-solid/icons/settings";
import { createSignal, For, Show, Suspense } from "solid-js";

import { EmptyState } from "@/components/ui/empty-state";
import { SegmentedControl } from "@/components/ui/segmented-control";

import { AgentPlaceholder } from "./agent-placeholder";
import { RefList } from "./ref-list";

// The Agents view is hidden until agent sessions exist; adding it back here brings back the switch.
const MODES: { label: string; icon: typeof ListTree }[] = [{ label: "List", icon: ListTree }];

export function RefsSidebar(props: { repositoryId: string; open: boolean }) {
  const [mode, setMode] = createSignal("List");

  return (
    <aside
      class={cn(
        "relative flex min-h-0 min-w-0 flex-col overflow-hidden border-r border-border bg-panel transition-opacity duration-150 motion-reduce:transition-none max-md:w-[52px]",
        !props.open && "pointer-events-none opacity-0",
      )}
    >
      <Show when={MODES.length > 1}>
        <div class="px-2.5 pt-[9px] pb-2 max-md:hidden">
          <SegmentedControl
            value={mode()}
            options={MODES.map((option) => option.label)}
            onChange={setMode}
          />
        </div>
        <RailModeSwitch value={mode()} onChange={setMode} />
      </Show>
      <Show when={mode() === "List"} fallback={<AgentPlaceholder />}>
        <nav
          class="flex min-h-0 flex-1 flex-col overflow-hidden pb-[35px] max-md:items-center max-md:gap-1 max-md:pt-2"
          aria-label="Repository references"
        >
          <Suspense
            fallback={
              <EmptyState icon={LoaderCircle} loading title="Loading branches…" class="h-[150px]" />
            }
          >
            <RefList repositoryId={props.repositoryId} />
          </Suspense>
        </nav>
      </Show>
      <button class="absolute right-0 bottom-0 left-0 flex h-[34px] cursor-pointer items-center gap-[7px] border-0 border-t border-border-soft bg-panel px-3 text-[11.5px] text-faint hover:text-text-soft max-md:justify-center max-md:px-0 max-md:[&>span]:hidden">
        <Settings size={15} />
        <span>Configure sidebar</span>
      </button>
    </aside>
  );
}

/** The List / Agents switch as a column of icons, for the narrow sidebar below 900px. */
function RailModeSwitch(props: { value: string; onChange: (value: string) => void }) {
  return (
    <div
      class="hidden flex-col items-center gap-1 border-b border-border-soft py-2 max-md:flex"
      role="group"
      aria-label="Sidebar view"
    >
      <For each={MODES}>
        {(option) => (
          <button
            class={cn(
              "grid size-9 cursor-pointer place-items-center rounded-lg border-0 bg-transparent text-muted hover:bg-panel-hover hover:text-text focus-ring-inset",
              props.value === option.label &&
                "bg-primary-soft text-primary-strong hover:bg-primary-soft hover:text-primary-strong",
            )}
            title={option.label}
            aria-label={option.label}
            aria-pressed={props.value === option.label}
            onClick={() => props.onChange(option.label)}
          >
            <option.icon size={16} />
          </button>
        )}
      </For>
    </div>
  );
}
