import type { FileStatus } from "@gitto/git/types";
import { cn } from "cn";

import { toneClasses, type Tone } from "@/components/ui/tone";

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

/** A file's status as a letter, like `M` for modified, in its colour. */
export function FileStatusBadge(props: { status: FileStatus }) {
  return (
    <span
      class={cn(
        "grid size-[17px] shrink-0 place-items-center rounded-sm font-mono text-[10.5px] font-bold",
        toneClasses[fileStatusTone[props.status]],
      )}
      title={props.status}
    >
      {fileStatusLabel[props.status]}
    </span>
  );
}
