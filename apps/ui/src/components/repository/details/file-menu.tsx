import type { ChangedFile } from "@gitto/git/types";
import type { LucideIcon } from "lucide-solid";
import { For, type JSX } from "solid-js";

import { ContextMenuItem } from "@/components/ui/context-menu";

import { RowMenu } from "../row-menu";

/** One of the actions in an uncommitted file's menu, like discarding its changes. */
export interface FileMenuAction {
  label: string;
  icon: LucideIcon;
  /** Whether it can't be done with `file` now. */
  disabled?: (file: ChangedFile) => boolean;
  run: (file: ChangedFile) => void;
}

/**
 * The menu of what can be done with one of `files`, opened by right-clicking its row in the list
 * `children` renders: one marked with its path as `data-file` (see `RowMenu`). Each of `actions` is
 * one of its items, in order.
 */
export function FileMenu(props: {
  repositoryId: string;
  files: ChangedFile[];
  actions: FileMenuAction[];
  /** Told which file's menu is open, by its path, if any, to show which one it's for. */
  onOpenFor: (path: string | undefined) => void;
  children: JSX.Element;
}) {
  return (
    <RowMenu
      repositoryId={props.repositoryId}
      attribute="file"
      item={(path) => props.files.find((file) => file.path === path)}
      onOpenFor={(path) => props.onOpenFor(path)}
      actions={(file) => (
        <For each={props.actions}>
          {(action) => (
            <ContextMenuItem
              icon={action.icon}
              label={action.label}
              disabled={action.disabled?.(file())}
              onSelect={() => action.run(file())}
            />
          )}
        </For>
      )}
    >
      {props.children}
    </RowMenu>
  );
}
