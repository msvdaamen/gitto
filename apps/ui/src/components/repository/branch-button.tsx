import { Popover } from "@kobalte/core/popover";
import GitBranch from "lucide-solid/icons/git-branch";
import { createEffect, createSignal, on, Show, Suspense } from "solid-js";

import { Button } from "@/components/ui/button";
import { FormField } from "@/components/ui/form-field";
import { useCreateBranch } from "@/git/queries/branch";
import { useStatus } from "@/git/queries/status";
import { branchBlocker, branchTitle, hasUncommittedChanges, headLabel } from "@/git/status";

import { ToolbarButton } from "./toolbar-button";

/**
 * Creates a branch from the current one and switches to it, with a popover to name it. Uncommitted
 * changes come along to the new branch.
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
  const create = useCreateBranch();
  const [open, setOpen] = createSignal(false);
  const [name, setName] = createSignal("");
  // The last creation, if it was in this repository; one in another stays with that repository.
  const current = () => create.variables?.repositoryId === props.repositoryId;
  const pending = () => current() && create.isPending;
  const error = () => (current() ? create.error : null);
  const canCreate = () => !!name().trim() && !pending();

  // The popover names a branch for the repository it was opened in.
  createEffect(
    on(
      () => props.repositoryId,
      () => setOpen(false),
      { defer: true },
    ),
  );

  function toggle(isOpen: boolean) {
    // A fresh name each time, and no error from the last try.
    if (isOpen && !pending()) {
      setName("");
      create.reset();
    }
    setOpen(isOpen);
  }

  function submit() {
    if (!canCreate()) return;
    create.mutate(
      { repositoryId: props.repositoryId, name: name().trim() },
      { onSuccess: () => setOpen(false) },
    );
  }

  return (
    <Popover open={open()} onOpenChange={toggle} placement="bottom-start" gutter={6}>
      <Popover.Trigger
        as={ToolbarButton}
        icon={GitBranch}
        label="Branch"
        hideBelow="sm"
        title={status.data ? branchTitle(status.data) : "Branch"}
        disabled={!status.data || !!branchBlocker(status.data) || pending()}
        busy={pending()}
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
              From {status.data ? headLabel(status.data.head) : "HEAD"}
              {hasUncommittedChanges(status.data) ? ", with your uncommitted changes." : "."}
            </Popover.Description>
            <FormField
              label="Name"
              placeholder="feature/my-change"
              value={name()}
              onChange={setName}
              readOnly={pending()}
            />
            <Button
              type="submit"
              variant="primary"
              icon={GitBranch}
              disabled={!canCreate()}
              class="h-8 w-full gap-[7px] rounded-[7px] text-[12.5px] font-[680] shadow-none"
            >
              {pending() ? "Creating…" : "Create branch"}
            </Button>
            <Show when={error()} keyed>
              {(failure) => (
                <p class="m-0 mt-2.5 text-[11.5px] break-words whitespace-pre-wrap text-coral">
                  {failure.message}
                </p>
              )}
            </Show>
          </form>
        </Popover.Content>
      </Popover.Portal>
    </Popover>
  );
}
