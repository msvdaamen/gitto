import type { ChangedFile } from "@gitto/git/types";
import Undo2 from "lucide-solid/icons/undo-2";
import type { JSX } from "solid-js";

import { ContextMenuItem } from "@/components/ui/context-menu";
import { isNestedRepository } from "@/git/changes";

import { RowMenu } from "../row-menu";

/**
 * The menu of what can be done with an uncommitted file, opened by right-clicking its row in the
 * list `children` renders: one marked with its path as `data-file` (see `RowMenu`). Discarding its
 * changes asks first, as they're lost. Not for a conflicted file, which is for resolving, nor a
 * repository inside this one, whose files aren't this one's.
 */
export function FileMenu(props: {
  repositoryId: string;
  /** The files in the list. */
  files: ChangedFile[];
  /** Whether its actions wait, as files are being staged or discarded. */
  busy: boolean;
  /** Told which file's menu is open, by its path, if any, to show which one it's for. */
  onOpenFor: (path: string | undefined) => void;
  onDiscard: (file: ChangedFile) => void;
  children: JSX.Element;
}) {
  return (
    <RowMenu
      repositoryId={props.repositoryId}
      attribute="file"
      item={(path) => props.files.find((file) => file.path === path)}
      onOpenFor={(path) => props.onOpenFor(path)}
      actions={(file) => (
        <ContextMenuItem
          icon={Undo2}
          label="Discard changes…"
          disabled={props.busy || file().status === "conflicted" || isNestedRepository(file())}
          onSelect={() => props.onDiscard(file())}
        />
      )}
    >
      {props.children}
    </RowMenu>
  );
}
