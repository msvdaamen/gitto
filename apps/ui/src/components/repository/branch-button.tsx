import { Popover } from "@kobalte/core/popover";
import GitBranch from "lucide-solid/icons/git-branch";
import { createEffect, createSignal, on, Suspense } from "solid-js";

import { useCreateBranch } from "@/git/queries/branch";
import { useStatus } from "@/git/queries/status";
import { branchSource, hasUncommittedChanges } from "@/git/status";

import { BranchForm } from "./branch-form";
import { FailurePopover } from "./failure-popover";
import { ToolbarButton } from "./toolbar-button";

/**
 * Creates a branch from the current one and switches to it, with a popover to name it, and says why
 * if that failed. Uncommitted changes come along to the new branch.
 */
export function BranchButton(props: { repositoryId: string }) {
  return (
    <Suspense fallback={<ToolbarButton icon={GitBranch} label="Branch" hideBelow="sm" disabled />}>
      <Branch repositoryId={props.repositoryId} />
    </Suspense>
  );
}

function Branch(props: { repositoryId: string }) {
  const status = useStatus(() => props.repositoryId);
  const create = useCreateBranch(() => props.repositoryId);
  const [open, setOpen] = createSignal(false);
  const [name, setName] = createSignal("");
  const source = () => (status.data ? branchSource(status.data.head) : "HEAD");
  const changes = () =>
    hasUncommittedChanges(status.data) ? ", with your uncommitted changes." : ".";

  // The popover names a branch for the repository it was opened in.
  createEffect(
    on(
      () => props.repositoryId,
      () => {
        setOpen(false);
        setName("");
      },
      { defer: true },
    ),
  );

  function toggle(isOpen: boolean) {
    // Out of the way of the name, which is kept after a failure to fix it.
    if (isOpen) create.dismiss();
    setOpen(isOpen);
  }

  function submit(trimmed: string) {
    // The button shows it running, and why it failed.
    setOpen(false);
    // Only the name that was used: another may be being typed in another repository by then.
    create.run(trimmed, {
      onSuccess: () => setName((typed) => (typed.trim() === trimmed ? "" : typed)),
    });
  }

  return (
    <FailurePopover title="Branch" error={create.error()} onDismiss={() => create.dismiss()}>
      <Popover open={open()} onOpenChange={toggle} placement="bottom-start" gutter={6}>
        <Popover.Trigger
          as={ToolbarButton}
          icon={GitBranch}
          label="Branch"
          hideBelow="sm"
          title={`Create a branch from ${source()}`}
          disabled={!status.data || create.isPending()}
          busy={create.isPending()}
        />
        <Popover.Portal>
          <Popover.Content class="z-50 w-[280px] animate-toast-in rounded-lg border border-border bg-panel-raised p-3 text-text shadow-app outline-none motion-reduce:animate-none">
            <BranchForm
              title={Popover.Title}
              description={Popover.Description}
              summary={`From ${source()}${changes()}`}
              name={name()}
              onNameChange={setName}
              onSubmit={submit}
            />
          </Popover.Content>
        </Popover.Portal>
      </Popover>
    </FailurePopover>
  );
}
