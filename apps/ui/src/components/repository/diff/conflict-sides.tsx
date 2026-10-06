import type { Conflict } from "@gitto/git/types";
import GitMergeConflict from "lucide-solid/icons/git-merge-conflict";
import { Show } from "solid-js";

import { Button } from "@/components/ui/button";
import { describeConflict, describeSide, keepLabel } from "@/git/conflicts";

/**
 * A conflict that's resolved by keeping one side whole: what each side has at the file's path,
 * and a button to keep it, e.g. "Delete, as theirs does".
 */
export function ConflictSides(props: {
  conflict: Conflict;
  disabled: boolean;
  onKeep: (side: "ours" | "theirs") => void;
}) {
  return (
    <div class="flex h-full flex-col items-center justify-center gap-4 px-6 text-center">
      <span class="flex flex-col items-center gap-1.5 text-faint">
        <GitMergeConflict size={22} />
        <strong class="text-[13.5px] text-text-soft">{describeConflict(props.conflict)}</strong>
        <Show when={props.conflict.unreadable}>
          {(reason) => <span class="text-[11.5px]">{reason()}</span>}
        </Show>
      </span>
      <div class="grid w-full max-w-[460px] grid-cols-2 gap-3">
        {(["ours", "theirs"] as const).map((side) => (
          <div class="flex flex-col items-center gap-2 rounded-lg border border-border bg-panel p-3">
            <span class="text-[10.5px] font-[700] tracking-[.07em] text-muted uppercase">
              {side}
            </span>
            <span class="text-[12px] text-text-soft">{describeSide(props.conflict[side])}</span>
            <Button disabled={props.disabled} onClick={() => props.onKeep(side)}>
              {keepLabel(props.conflict, side)}
            </Button>
          </div>
        ))}
      </div>
    </div>
  );
}
