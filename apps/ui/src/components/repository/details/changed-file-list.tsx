import type { ChangedFile } from "@gitto/git/types";
import { cn } from "cn";
import type { LucideIcon } from "lucide-solid";
import { onCleanup, Show } from "solid-js";
import { Dynamic } from "solid-js/web";

import { FileStatusBadge } from "@/components/ui/file-status-badge";
import { LineStats } from "@/components/ui/line-stats";
import { VirtualRows } from "@/components/ui/virtual-list";

/** An action shown on each file when it's hovered, like staging it. */
export interface FileAction {
  label: string;
  icon: LucideIcon;
  disabled?: boolean;
  run: (file: ChangedFile) => void;
}

/** How long the pointer rests on a file before it's taken for one about to be opened. */
const HOVER_MS = 80;

/** Height of a file's row, and the space below it. */
const ROW_HEIGHT = 42;
const ROW_GAP = 2;

/**
 * The files, in `scrollElement`, which scrolls them along with whatever is around them. Only the
 * ones in view are rendered: a commit or the working tree can have tens of thousands.
 */
export function ChangedFileList(props: {
  files: ChangedFile[];
  /** The files' lines weren't counted, so one without line counts isn't known to be binary. */
  uncounted?: boolean;
  scrollElement: HTMLElement | undefined;
  action?: FileAction;
  /** Shows a file's changes when it's clicked. */
  onOpen?: (file: ChangedFile) => void;
  /** The path of the file whose changes are on show, if one's in the list. */
  openPath?: string;
  /** Loads a file's changes ahead, once the pointer rests on it: it's likely to be opened. */
  onPrefetch?: (file: ChangedFile) => void;
  /** The conflicted files with no conflict markers left, which are badged as ready. */
  markerFree?: ReadonlySet<string>;
  /** The path of the file shown as if hovered, e.g. while its menu is open. */
  highlightedPath?: string;
}) {
  let hover: ReturnType<typeof setTimeout> | undefined;
  onCleanup(() => clearTimeout(hover));
  return (
    <VirtualRows
      items={props.files}
      rowHeight={ROW_HEIGHT}
      gap={ROW_GAP}
      scrollElement={props.scrollElement}
    >
      {(file) => (
        <div
          // Marks it for a menu of what can be done with it, if the list has one (see `RowMenu`).
          data-file={file().path}
          class={cn(
            "group flex h-full w-full items-center rounded-md hover:bg-panel-hover",
            props.highlightedPath === file().path && "bg-panel-hover",
            props.openPath === file().path && "bg-panel-active hover:bg-panel-active",
          )}
        >
          {/* A button when the file opens, so its contents (the status's tooltip, say) still
              take the pointer; its action is next to it, rather than inside. */}
          <Dynamic
            component={props.onOpen ? "button" : "div"}
            type={props.onOpen ? "button" : undefined}
            class={cn(
              "grid h-full min-w-0 flex-1 grid-cols-[18px_minmax(0,1fr)_auto] items-center gap-[7px] rounded-md border-0 bg-transparent p-[7px] text-left",
              props.onOpen && "cursor-pointer focus-ring-inset",
            )}
            aria-label={props.onOpen ? `Show changes in ${file().path}` : undefined}
            aria-current={(props.onOpen && props.openPath === file().path) || undefined}
            onClick={props.onOpen && (() => props.onOpen?.(file()))}
            onPointerEnter={() => {
              clearTimeout(hover);
              if (props.onPrefetch) hover = setTimeout(() => props.onPrefetch?.(file()), HOVER_MS);
            }}
            onPointerLeave={() => clearTimeout(hover)}
          >
            <FileStatusBadge status={file().status} ready={props.markerFree?.has(file().path)} />
            <span class="flex min-w-0 flex-col gap-0.5">
              <strong class="truncate text-[12px] font-[540]">
                {file().path.split("/").slice(-1)[0]}
              </strong>
              <small class="truncate text-[10.5px] text-faint">
                {file().path.includes("/")
                  ? file().path.slice(0, file().path.lastIndexOf("/"))
                  : "root"}
              </small>
            </span>
            <span class="flex items-center text-[10.5px]">
              <Show
                when={file().additions !== null}
                fallback={
                  <em class="text-faint not-italic">
                    {file().status === "untracked" ? "new" : props.uncounted ? "" : "binary"}
                  </em>
                }
              >
                <LineStats additions={file().additions ?? 0} deletions={file().deletions ?? 0} />
              </Show>
            </span>
          </Dynamic>
          <Show when={props.action}>
            {(action) => (
              <button
                class="mr-[7px] -ml-0.5 grid size-[20px] shrink-0 cursor-pointer place-items-center rounded-[5px] border border-border bg-panel-raised p-0 text-text-soft opacity-0 group-focus-within:opacity-100 group-hover:opacity-100 hover:border-[color-mix(in_srgb,var(--primary)_45%,var(--border))] hover:text-primary-strong focus-visible:opacity-100 disabled:cursor-default group-hover:disabled:opacity-50"
                aria-label={`${action().label} ${file().path}`}
                title={action().label}
                disabled={action().disabled}
                onClick={() => action().run(file())}
              >
                <Dynamic component={action().icon} size={12} strokeWidth={2} />
              </button>
            )}
          </Show>
        </div>
      )}
    </VirtualRows>
  );
}
