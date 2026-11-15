import { Dialog } from "@kobalte/core/dialog";
import { cn } from "cn";
import { createEffect, createSignal, on } from "solid-js";

import { DIALOG_BOX, DialogPortal } from "@/components/ui/dialog";
import { useCreateBranchFrom } from "@/git/queries/branch";
import { useStatus } from "@/git/queries/status";
import { branchName, hasUncommittedChanges, headSha, isCheckedOut } from "@/git/status";
import { useLingering } from "@/hooks/lingering";
import { useResetOn } from "@/hooks/reset-on";

import { BranchForm } from "./branch-form";

/** Whether `from`, what a branch is made from, is a branch's full ref name, rather than a SHA. */
const isRefName = (from: string) => from.startsWith("refs/");

/**
 * Names a branch to create from `from`, a branch by its full ref name or a commit by its full SHA,
 * and switches to it, taking the uncommitted changes along; open while there's a `from`. That's a
 * switch of branches: the current branch, in the toolbar, shows it running, and why it failed. It
 * can't be created while another switch, or a merge, is running.
 */
export function CreateBranchDialog(props: {
  repositoryId: string;
  from: string | undefined;
  onClose: () => void;
}) {
  const status = useStatus(() => props.repositoryId);
  const create = useCreateBranchFrom(() => props.repositoryId);
  const [name, setName] = createSignal("");
  const from = useLingering(() => props.from);

  /** Where the uncommitted changes go: from another commit than HEAD, they can conflict with it. */
  const changes = () => {
    if (!hasUncommittedChanges(status.data)) return ".";
    const head = status.data?.head;
    const source = from() ?? "";
    const atHead = isRefName(source)
      ? !!head && isCheckedOut(head, source)
      : !!head && headSha(head) === source;
    if (atHead) return ", with your uncommitted changes.";
    return ", with your uncommitted changes, unless they conflict with it: then they're kept in the stash.";
  };

  // A name typed in one repository isn't for another.
  useResetOn(
    () => props.repositoryId,
    () => setName(""),
  );
  // Out of the way of the name, which is kept after a failure to fix it.
  createEffect(
    on(
      () => !!props.from,
      (open) => open && create.dismiss(),
    ),
  );

  function submit(trimmed: string) {
    const ref = from()!;
    props.onClose();
    // Only the name that was used: another may be being typed in another repository by then.
    create.run(
      { name: trimmed, from: ref },
      { onSuccess: () => setName((typed) => (typed.trim() === trimmed ? "" : typed)) },
    );
  }

  return (
    <Dialog open={!!props.from} onOpenChange={(open) => !open && props.onClose()} modal>
      <DialogPortal>
        <Dialog.Content class={cn(DIALOG_BOX, "max-w-[320px] p-4")}>
          <BranchForm
            title={Dialog.Title}
            description={Dialog.Description}
            summary={`From ${sourceLabel(from() ?? "")}${changes()}`}
            // `run` does nothing while a switch or a merge is running.
            disabled={create.isBlocked()}
            name={name()}
            onNameChange={setName}
            onSubmit={submit}
          />
        </Dialog.Content>
      </DialogPortal>
    </Dialog>
  );
}

/** What a branch is made from, `from`, as the dialog names it: `main`, or `commit 1a2b3c4`. */
function sourceLabel(from: string): string {
  return isRefName(from) ? branchName(from) : `commit ${from.slice(0, 7)}`;
}
