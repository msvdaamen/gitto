import { Popover } from "@kobalte/core/popover";
import GitBranch from "lucide-solid/icons/git-branch";
import { createEffect, createSignal, on, Suspense } from "solid-js";

import { Button } from "@/components/ui/button";
import { FormField } from "@/components/ui/form-field";
import { useCreateBranch } from "@/git/queries/branch";
import { useStatus } from "@/git/queries/status";
import { branchSource, hasUncommittedChanges } from "@/git/status";

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

  function submit() {
    const trimmed = name().trim();
    if (!trimmed) return;
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
            <form
              onSubmit={(event) => {
                event.preventDefault();
                submit();
              }}
            >
              <Popover.Title class="m-0 text-[12.5px] font-[680]">New branch</Popover.Title>
              <Popover.Description class="m-0 mt-1 mb-2.5 text-[11.5px] leading-[1.45] text-muted">
                From {source()}
                {hasUncommittedChanges(status.data) ? ", with your uncommitted changes." : "."}
              </Popover.Description>
              <FormField
                label="Name"
                placeholder="feature/my-change"
                value={name()}
                onChange={setName}
              />
              <Button
                type="submit"
                variant="primary"
                icon={GitBranch}
                disabled={!name().trim()}
                class="h-8 w-full gap-[7px] rounded-[7px] text-[12.5px] font-[680] shadow-none"
              >
                Create branch
              </Button>
            </form>
          </Popover.Content>
        </Popover.Portal>
      </Popover>
    </FailurePopover>
  );
}
