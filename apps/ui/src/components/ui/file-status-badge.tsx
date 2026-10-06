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

/**
 * A file's status as a letter, like `M` for modified, in its colour. A conflicted file that's
 * `ready`, with no conflict markers left, has a check instead, until it's marked resolved.
 */
export function FileStatusBadge(props: { status: FileStatus; ready?: boolean }) {
  const ready = () => props.status === "conflicted" && props.ready;
  return (
    <span
      class={cn(
        "grid size-[17px] shrink-0 place-items-center rounded-sm font-mono text-[10.5px] font-bold",
        toneClasses[ready() ? "mint" : fileStatusTone[props.status]],
      )}
      title={ready() ? "conflicted, but resolved: ready to mark resolved" : props.status}
    >
      {ready() ? "✓" : fileStatusLabel[props.status]}
    </span>
  );
}
