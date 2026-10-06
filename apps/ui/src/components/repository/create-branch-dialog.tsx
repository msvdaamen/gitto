import { Dialog } from "@kobalte/core/dialog";
import { cn } from "cn";
import { createEffect, createMemo, createSignal, on } from "solid-js";

import { DIALOG_BOX, DialogPortal } from "@/components/ui/dialog";
import { useCreateBranchFrom } from "@/git/queries/branch";
import { useStatus } from "@/git/queries/status";
import { branchName, hasUncommittedChanges, isCheckedOut } from "@/git/status";

import { BranchForm } from "./branch-form";

/**
 * Names a branch to create from the branch `from`, by its full ref name, and switches to it, taking the uncommitted
 * changes along; open while there's a `from`. That's a switch of branches: the current branch, in
 * the toolbar, shows it running, and why it failed. It can't be created while another switch, or a
 * merge, is running.
 */
export function CreateBranchDialog(props: {
  repositoryId: string;
  from: string | undefined;
  onClose: () => void;
}) {
  const status = useStatus(() => props.repositoryId);
  const create = useCreateBranchFrom(() => props.repositoryId);
  const [name, setName] = createSignal("");
  // The last one while it closes, rather than none.
  const from = createMemo<string | undefined>((last) => props.from ?? last);

  /** Where the uncommitted changes go: from another commit than HEAD, they can conflict with it. */
  const changes = () => {
    if (!hasUncommittedChanges(status.data)) return ".";
    const head = status.data?.head;
    if (head && isCheckedOut(head, from() ?? "")) return ", with your uncommitted changes.";
    return ", with your uncommitted changes, unless they conflict with it: then they're kept in the stash.";
  };

  // A name typed in one repository isn't for another.
  createEffect(
    on(
      () => props.repositoryId,
      () => setName(""),
      { defer: true },
    ),
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
            summary={`From ${branchName(from() ?? "")}${changes()}`}
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
