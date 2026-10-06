import { Popover } from "@kobalte/core/popover";
import { cn } from "cn";
import type { LucideIcon } from "lucide-solid";
import ChevronRight from "lucide-solid/icons/chevron-right";
import LoaderCircle from "lucide-solid/icons/loader-circle";
import { Show } from "solid-js";
import type { JSX } from "solid-js";
import { Dynamic } from "solid-js/web";

import { VirtualList } from "@/components/ui/virtual-list";
import { compactCount } from "@/lib/format";
import type { ScrollId } from "@/lib/scroll";

import { ROW_HEIGHT } from "./sidebar-row";

/** Height of a section's header, in pixels. */
const HEADER_HEIGHT = 30;
/** Space above a section's first row and below its last one. */
const LIST_PADDING = 4;
/**
 * Height of an expanded section without its rows: its top border, its header, the border below
 * that and the list's padding.
 */
const SECTION_CHROME = 1 + HEADER_HEIGHT + 1 + 2 * LIST_PADDING;

/**
 * A section of the sidebar, like GitKraken's: its header always shows, and while expanded its rows
 * scroll on their own, only rendering the ones in view. Expanded sections share the sidebar's
 * height, but never take more than their rows need. Below 900px the section is an icon in a rail
 * instead, that opens its rows in a popover.
 */
export function SidebarSection<T>(props: {
  /** The section's element, e.g. to place a popover by. */
  ref?: (element: HTMLElement) => void;
  title: string;
  /** Marks the rows' list, so it starts at the top again in another repository (`SCROLL_IDS`). */
  scrollId: ScrollId;
  icon: LucideIcon;
  /** Running something on its items, e.g. a pop of a stash: a spinner takes the icon's place. */
  busy?: boolean;
  count: number;
  collapsed: boolean;
  onToggle: () => void;
  items: T[];
  /** Renders a row; its item can change, as rows are rendered by position (see `VirtualList`). */
  children: (item: () => T) => JSX.Element;
}) {
  const expanded = () => !props.collapsed && props.items.length > 0;

  return (
    <section
      ref={props.ref}
      aria-busy={props.busy}
      class={cn(
        // At least its top border and its header.
        "flex min-h-[calc(var(--header-height)+1px)] flex-col border-t border-border-soft max-md:min-h-0 max-md:flex-none max-md:border-0",
        expanded() ? "flex-1" : "flex-none",
      )}
      style={{
        "--header-height": `${HEADER_HEIGHT}px`,
        "max-height": expanded()
          ? `${SECTION_CHROME + props.items.length * ROW_HEIGHT}px`
          : undefined,
      }}
    >
      <button
        class={cn(
          "flex h-(--header-height) w-full shrink-0 cursor-pointer items-center gap-1.5 border-0 bg-panel-raised px-2.5 text-left text-muted hover:bg-panel-hover hover:text-text-soft focus-ring-inset max-md:hidden",
          !props.collapsed && "text-text-soft",
        )}
        aria-expanded={!props.collapsed ? "true" : "false"}
        onClick={props.onToggle}
      >
        <ChevronRight
          size={12}
          class={cn(
            "shrink-0 text-faint transition-transform duration-150 motion-reduce:transition-none",
            !props.collapsed && "rotate-90",
          )}
        />
        <SectionIcon icon={props.icon} busy={props.busy} size={13} />
        <SectionTitle title={props.title} count={props.count} />
      </button>
      <Show when={expanded()}>
        <VirtualList
          items={props.items}
          rowHeight={ROW_HEIGHT}
          padding={LIST_PADDING}
          scrollId={props.scrollId}
          class="border-t border-border-soft px-1.5 max-md:hidden"
        >
          {props.children}
        </VirtualList>
      </Show>
      <RailSection
        title={props.title}
        icon={props.icon}
        busy={props.busy}
        count={props.count}
        items={props.items}
      >
        {props.children}
      </RailSection>
    </section>
  );
}

/** A section's icon, or a spinner in its place while it's busy. */
function SectionIcon(props: { icon: LucideIcon; busy?: boolean; size: number }) {
  return (
    <Dynamic
      component={props.busy ? LoaderCircle : props.icon}
      size={props.size}
      class={cn("shrink-0", props.busy && "animate-spin motion-reduce:animate-none")}
    />
  );
}

/** A section's title, with how many items it has. */
function SectionTitle(props: { title: string; count: number }) {
  return (
    <>
      <span class="min-w-0 flex-1 truncate text-[11.5px] font-[720] tracking-[.06em] uppercase">
        {props.title}
      </span>
      <span class="shrink-0 rounded-full bg-bg px-1.5 py-px text-[10.5px] font-[650] text-faint tabular-nums">
        {props.count}
      </span>
    </>
  );
}

/**
 * A section in the narrow sidebar: its icon, with a badge counting its items, that opens the
 * section's rows in a popover beside the rail.
 */
function RailSection<T>(props: {
  title: string;
  icon: LucideIcon;
  busy?: boolean;
  count: number;
  items: T[];
  children: (item: () => T) => JSX.Element;
}) {
  return (
    <Popover placement="right-start" gutter={10}>
      <Popover.Trigger
        class="relative hidden size-9 cursor-pointer place-items-center rounded-lg border-0 bg-transparent text-muted hover:bg-panel-hover hover:text-text focus-ring-inset data-expanded:bg-primary-soft data-expanded:text-primary-strong max-md:grid"
        title={props.title}
        aria-label={`${props.title} (${props.count})`}
      >
        <SectionIcon icon={props.icon} busy={props.busy} size={16} />
        <Show when={props.count > 0}>
          <span class="absolute -top-0.5 -right-1 min-w-[16px] rounded-full border-2 border-panel bg-panel-active px-[3px] text-center text-[10px] leading-[11px] font-[700] text-text-soft tabular-nums">
            {compactCount(props.count)}
          </span>
        </Show>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content class="z-50 flex w-[260px] flex-col overflow-hidden rounded-lg border border-border bg-panel shadow-app outline-none">
          <Popover.Title
            class="flex shrink-0 items-center gap-1.5 border-b border-border-soft bg-panel-raised px-2.5 text-text-soft"
            // Outside the section, at the end of the page, so not through its `--header-height`.
            style={{ height: `${HEADER_HEIGHT}px` }}
          >
            <props.icon size={13} class="shrink-0" />
            <SectionTitle title={props.title} count={props.count} />
          </Popover.Title>
          <Show
            when={props.items.length}
            fallback={
              <p class="px-3 py-4 text-center text-[12.5px] text-faint">Nothing here yet</p>
            }
          >
            <VirtualList
              items={props.items}
              rowHeight={ROW_HEIGHT}
              padding={LIST_PADDING}
              class="max-h-[min(400px,70vh)] px-1.5"
            >
              {props.children}
            </VirtualList>
          </Show>
        </Popover.Content>
      </Popover.Portal>
    </Popover>
  );
}
