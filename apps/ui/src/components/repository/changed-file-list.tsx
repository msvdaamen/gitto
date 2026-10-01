import type { ChangedFile, FileStatus } from "@gitto/git/types";
import { cn } from "cn";
import type { LucideIcon } from "lucide-solid";
import { Show } from "solid-js";
import { Dynamic } from "solid-js/web";

import { LineStats } from "@/components/ui/line-stats";
import { toneClasses, type Tone } from "@/components/ui/tone";
import { VirtualRows } from "@/components/ui/virtual-list";

const fileStatusLabel: Record<FileStatus, string> = {
  modified: "M",
  added: "A",
  deleted: "D",
  renamed: "R",
  copied: "C",
  typechange: "T",
  untracked: "U",
  conflicted: "!",
};

const fileStatusTone: Record<FileStatus, Tone> = {
  modified: "amber",
  added: "mint",
  deleted: "coral",
  renamed: "blue",
  copied: "blue",
  typechange: "amber",
  untracked: "mint",
  conflicted: "coral",
};

/** An action shown on each file when it's hovered, like staging it. */
export interface FileAction {
  label: string;
  icon: LucideIcon;
  disabled?: boolean;
  run: (file: ChangedFile) => void;
}

/** Height of a file's row (`h-[42px]`), plus the space below it. */
const ROW_HEIGHT = 42 + 2;

/**
 * The files, in `scrollElement`, which scrolls them along with whatever is around them. Only the
 * ones in view are rendered: a commit or the working tree can have tens of thousands.
 */
export function ChangedFileList(props: {
  files: ChangedFile[];
  scrollElement: HTMLElement | undefined;
  action?: FileAction;
}) {
  return (
    <VirtualRows items={props.files} rowHeight={ROW_HEIGHT} scrollElement={props.scrollElement}>
      {(file) => (
        <div class="group grid h-[42px] w-full grid-cols-[18px_minmax(0,1fr)_auto] items-center gap-[7px] rounded-md p-[7px] text-left hover:bg-panel-hover">
          <span
            class={cn(
              "grid size-[17px] place-items-center rounded-sm font-mono text-[8px] font-bold",
              toneClasses[fileStatusTone[file().status]],
            )}
            title={file().status}
          >
            {fileStatusLabel[file().status]}
          </span>
          <span class="flex min-w-0 flex-col gap-0.5">
            <strong class="truncate text-[9.5px] font-[540]">
              {file().path.split("/").slice(-1)[0]}
            </strong>
            <small class="truncate text-[8px] text-faint">
              {file().path.includes("/")
                ? file().path.slice(0, file().path.lastIndexOf("/"))
                : "root"}
            </small>
          </span>
          <span class="flex items-center gap-[5px] text-[8px]">
            <Show
              when={file().additions !== null}
              fallback={
                <em class="text-faint not-italic">
                  {file().status === "untracked" ? "new" : "binary"}
                </em>
              }
            >
              <LineStats additions={file().additions ?? 0} deletions={file().deletions ?? 0} />
            </Show>
            <Show when={props.action}>
              {(action) => (
                <button
                  class="-my-1 ml-0.5 grid size-[20px] cursor-pointer place-items-center rounded-[5px] border border-border bg-panel-raised p-0 text-text-soft opacity-0 group-focus-within:opacity-100 group-hover:opacity-100 hover:border-[color-mix(in_srgb,var(--primary)_45%,var(--border))] hover:text-primary-strong focus-visible:opacity-100 disabled:cursor-default group-hover:disabled:opacity-50"
                  aria-label={`${action().label} ${file().path}`}
                  title={action().label}
                  disabled={action().disabled}
                  onClick={() => action().run(file())}
                >
                  <Dynamic component={action().icon} size={12} strokeWidth={2} />
                </button>
              )}
            </Show>
          </span>
        </div>
      )}
    </VirtualRows>
  );
}
