import { cn } from "cn";
import type { LucideIcon } from "lucide-solid";
import ChevronDown from "lucide-solid/icons/chevron-down";
import ChevronRight from "lucide-solid/icons/chevron-right";
import Folder from "lucide-solid/icons/folder";
import FolderOpen from "lucide-solid/icons/folder-open";
import { splitProps, type JSX } from "solid-js";

import { toneClasses } from "@/components/ui/tone";

/** Space below every row in a section, in pixels. */
const ROW_GAP = 1;
/** Height of every row in a section, including the gap below it. */
export const ROW_HEIGHT = 28 + ROW_GAP;
/** A row's own height, set from `ROW_HEIGHT` so the two can't drift apart. */
const ROW_BOX = `${ROW_HEIGHT - ROW_GAP}px`;
/** Indentation per tree level, in pixels. */
const DEPTH_INDENT = 12;

/** A folder in a ref tree, which collapses the refs below it. */
export function SidebarFolder(props: {
  name: string;
  count: number;
  depth: number;
  collapsed: boolean;
  onToggle: () => void;
}) {
  return (
    <button
      class="grid w-full cursor-pointer grid-cols-[12px_16px_minmax(0,1fr)_auto] items-center gap-[5px] rounded-[5px] border-0 bg-transparent pr-[7px] text-left text-muted hover:bg-panel-hover hover:text-text-soft focus-ring-inset"
      style={{ height: ROW_BOX, "padding-left": `${8 + props.depth * DEPTH_INDENT}px` }}
      aria-expanded={!props.collapsed ? "true" : "false"}
      title={props.name}
      onClick={props.onToggle}
    >
      {props.collapsed ? <ChevronRight size={12} /> : <ChevronDown size={12} />}
      {props.collapsed ? <Folder size={13} /> : <FolderOpen size={13} />}
      <span class="truncate text-[12.5px]">{props.name}</span>
      <small class="text-[10.5px] text-faint">{props.count}</small>
    </button>
  );
}

/**
 * An item in a section: a ref or a stash. Other attributes go to the button, e.g. one marking it
 * for a menu (see `BranchMenu`).
 */
export function SidebarRow(
  props: Omit<JSX.ButtonHTMLAttributes<HTMLButtonElement>, "title" | "onDblClick"> & {
    icon: LucideIcon;
    label: string;
    title?: string;
    depth?: number;
    active?: boolean;
    /** Shown as if hovered, e.g. while its menu is open. */
    highlighted?: boolean;
    count?: number;
    meta?: string;
    tone?: "amber";
    onDblClick?: () => void;
  },
) {
  const [local, others] = splitProps(props, [
    "icon",
    "label",
    "title",
    "depth",
    "active",
    "highlighted",
    "count",
    "meta",
    "tone",
    "onDblClick",
  ]);
  return (
    <button
      {...others}
      class={cn(
        "grid w-full cursor-pointer grid-cols-[16px_minmax(0,1fr)_auto_auto] items-center gap-[5px] rounded-[5px] border-0 bg-transparent pr-[7px] text-left text-muted hover:bg-panel-hover hover:text-text-soft focus-ring-inset",
        // The active one stands out already.
        local.active
          ? "bg-primary-soft text-text [&>svg]:text-primary-strong"
          : local.highlighted && "bg-panel-hover text-text-soft",
      )}
      style={{
        height: ROW_BOX,
        "padding-left": `${25 + (local.depth ?? 0) * DEPTH_INDENT}px`,
      }}
      title={local.title ?? local.label}
      // Read on each double-click: Solid binds a handler once.
      onDblClick={() => local.onDblClick?.()}
    >
      <local.icon size={13} />
      <span class="truncate text-[12.5px]">{local.label}</span>
      {local.meta && <small class="text-[10.5px] text-blue">{local.meta}</small>}
      {local.count !== undefined && (
        <em
          class={cn(
            "min-w-[17px] rounded-lg px-1 py-0.5 text-center text-[10.5px] not-italic",
            toneClasses[local.tone ?? "neutral"],
          )}
        >
          {local.count}
        </em>
      )}
    </button>
  );
}
