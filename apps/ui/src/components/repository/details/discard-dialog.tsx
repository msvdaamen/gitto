import Undo2 from "lucide-solid/icons/undo-2";
import { createMemo } from "solid-js";

import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import type { DiscardTarget } from "@/git/queries/staging";

/**
 * What the dialog asks about: the changes to discard and, for a file's unstaged ones, whether it
 * has staged ones too, `alsoStaged`, which it's put back as; else as the last commit has it.
 */
export type DiscardPrompt = "all" | (Exclude<DiscardTarget, "all"> & { alsoStaged: boolean });

/**
 * Asks before discarding changes, `target`, which are lost; open while there are some to ask
 * about. Esc, or clicking outside, cancels.
 */
export function DiscardDialog(props: {
  target: DiscardPrompt | undefined;
  onCancel: () => void;
  onDiscard: (target: DiscardTarget) => void;
}) {
  // The last ones asked about, still named while the dialog closes.
  const target = createMemo((last: DiscardPrompt | undefined) => props.target ?? last);
  const title = () => {
    const current = target();
    if (current === "all") return "Discard all changes?";
    const path = current?.file.path ?? "";
    return `Discard changes to ${path.slice(path.lastIndexOf("/") + 1)}?`;
  };
  return (
    <ConfirmDialog
      open={props.target !== undefined}
      icon={Undo2}
      tone="coral"
      title={title()}
      description={describe(target())}
      onCancel={props.onCancel}
    >
      <Button variant="ghost" onClick={props.onCancel}>
        Cancel
      </Button>
      <Button variant="primary" onClick={() => props.target && props.onDiscard(props.target)}>
        Discard
      </Button>
    </ConfirmDialog>
  );
}

/** What discarding `target` does, as the dialog says it. */
function describe(target: DiscardPrompt | undefined): string {
  if (target === undefined) return "";
  if (target === "all") {
    return "Every staged and unstaged change is lost, and untracked files are deleted. Ignored files are kept, and so are submodules' changes.";
  }
  const { side, file } = target;
  if (side === "unstaged") {
    // Added or renamed, the file was only marked to be added (`add -N`), and isn't staged.
    if (file.status === "untracked") return `${file.path} isn't tracked, so it's deleted.`;
    if (file.status === "added") return `${file.path} isn't staged yet, so it's deleted.`;
    // What's left of it: what's staged, which is what the last commit has if nothing is.
    const kept = target.alsoStaged ? "as it's staged" : "as the last commit has it";
    if (file.status === "renamed" && file.origPath) {
      return `${file.path} is deleted, and ${file.origPath} put back ${kept}.`;
    }
    if (file.status === "deleted") return `${file.path} is put back ${kept}.`;
    return `The unstaged changes to ${file.path} are lost.${target.alsoStaged ? " What's staged of it is kept." : ""}`;
  }
  // A copy is new too: the file it's a copy of is left as it is.
  if (file.status === "added" || file.status === "copied") {
    return `${file.path} is new, so it's deleted, with its staged and unstaged changes.`;
  }
  if (file.status === "renamed" && file.origPath) {
    return `${file.path} is deleted, and ${file.origPath} put back as the last commit has it: the staged and unstaged changes are lost.`;
  }
  return `${file.path} is put back as the last commit has it: its staged and unstaged changes are lost.`;
}
