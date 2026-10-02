import { cn } from "cn";
import type { LucideIcon } from "lucide-solid";
import ChevronDown from "lucide-solid/icons/chevron-down";
import ChevronRight from "lucide-solid/icons/chevron-right";
import Folder from "lucide-solid/icons/folder";
import FolderOpen from "lucide-solid/icons/folder-open";

import { toneClasses } from "@/components/ui/tone";

/** Height of every row in a section: 28px (`h-7`) plus the 1px gap below it. */
export const ROW_HEIGHT = 29;
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
      class="grid h-7 w-full cursor-pointer grid-cols-[12px_16px_minmax(0,1fr)_auto] items-center gap-[5px] rounded-[5px] border-0 bg-transparent pr-[7px] text-left text-muted hover:bg-panel-hover hover:text-text-soft focus-ring-inset"
      style={{ "padding-left": `${8 + props.depth * DEPTH_INDENT}px` }}
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

/** An item in a section: a ref, or the working directory, a pull request or a stash. */
export function SidebarRow(props: {
  icon: LucideIcon;
  label: string;
  title?: string;
  depth?: number;
  active?: boolean;
  count?: number;
  meta?: string;
  tone?: "amber";
}) {
  return (
    <button
      class={cn(
        "grid h-7 w-full cursor-pointer grid-cols-[16px_minmax(0,1fr)_auto_auto] items-center gap-[5px] rounded-[5px] border-0 bg-transparent pr-[7px] text-left text-muted hover:bg-panel-hover hover:text-text-soft focus-ring-inset",
        props.active && "bg-primary-soft text-text [&>svg]:text-primary-strong",
      )}
      style={{ "padding-left": `${25 + (props.depth ?? 0) * DEPTH_INDENT}px` }}
      title={props.title ?? props.label}
    >
      <props.icon size={13} />
      <span class="truncate text-[12.5px]">{props.label}</span>
      {props.meta && <small class="text-[10.5px] text-blue">{props.meta}</small>}
      {props.count !== undefined && (
        <em
          class={cn(
            "min-w-[17px] rounded-lg px-1 py-0.5 text-center text-[10.5px] not-italic",
            toneClasses[props.tone ?? "neutral"],
          )}
        >
          {props.count}
        </em>
      )}
    </button>
  );
}
