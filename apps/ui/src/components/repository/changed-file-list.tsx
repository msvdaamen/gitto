import type { ChangedFile, FileStatus } from "@gitto/git/types";
import type { LucideIcon } from "lucide-solid";
import { For, Show } from "solid-js";
import { Dynamic } from "solid-js/web";

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

/** An action shown on each file when it's hovered, like staging it. */
export interface FileAction {
  label: string;
  icon: LucideIcon;
  disabled?: boolean;
  run: (file: ChangedFile) => void;
}

export function ChangedFileList(props: { files: ChangedFile[]; action?: FileAction }) {
  return (
    <div class="flex flex-col gap-0.5">
      <For each={props.files}>
        {(file) => (
          <div class="group grid w-full grid-cols-[18px_minmax(0,1fr)_auto] items-center gap-[7px] rounded-md p-[7px] text-left hover:bg-panel-hover">
            <span
              class={`grid size-[17px] place-items-center rounded-sm font-mono text-[8px] font-bold ${
                file.status === "added" || file.status === "untracked"
                  ? "bg-mint-soft text-mint"
                  : file.status === "deleted" || file.status === "conflicted"
                    ? "bg-coral-soft text-coral"
                    : file.status === "renamed" || file.status === "copied"
                      ? "bg-blue-soft text-blue"
                      : "bg-amber-soft text-amber"
              }`}
              title={file.status}
            >
              {fileStatusLabel[file.status]}
            </span>
            <span class="flex min-w-0 flex-col gap-0.5">
              <strong class="truncate text-[9.5px] font-[540]">
                {file.path.split("/").slice(-1)[0]}
              </strong>
              <small class="truncate text-[8px] text-faint">
                {file.path.includes("/") ? file.path.slice(0, file.path.lastIndexOf("/")) : "root"}
              </small>
            </span>
            <span class="flex items-center gap-[5px] text-[8px]">
              <Show
                when={file.additions !== null}
                fallback={
                  <em class="text-faint not-italic">
                    {file.status === "untracked" ? "new" : "binary"}
                  </em>
                }
              >
                <em class="text-mint not-italic">+{file.additions}</em>
                <b class="font-medium text-coral">−{file.deletions}</b>
              </Show>
              <Show when={props.action}>
                {(action) => (
                  <button
                    class="-my-1 ml-0.5 grid size-[20px] cursor-pointer place-items-center rounded-[5px] border border-border bg-panel-raised p-0 text-text-soft opacity-0 group-focus-within:opacity-100 group-hover:opacity-100 hover:border-[color-mix(in_srgb,var(--primary)_45%,var(--border))] hover:text-primary-strong focus-visible:opacity-100 disabled:cursor-default group-hover:disabled:opacity-50"
                    aria-label={`${action().label} ${file.path}`}
                    title={action().label}
                    disabled={action().disabled}
                    onClick={() => action().run(file)}
                  >
                    <Dynamic component={action().icon} size={12} strokeWidth={2} />
                  </button>
                )}
              </Show>
            </span>
          </div>
        )}
      </For>
    </div>
  );
}
