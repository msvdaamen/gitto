import type { ChangedFile, Operation } from "@gitto/git/types";
import { AlertDialog } from "@kobalte/core/alert-dialog";
import { cn } from "cn";
import GitMergeConflict from "lucide-solid/icons/git-merge-conflict";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import { createMemo, createSignal, Show } from "solid-js";

import { Button } from "@/components/ui/button";
import { DIALOG_BOX, DialogPortal } from "@/components/ui/dialog";
import { FailurePopover } from "@/components/ui/failure-popover";
import { operationName, operationProgress, operationTitle } from "@/git/conflicts";
import { useOperationActions, useOperationInProgress } from "@/git/queries/progress";
import { useConflictedFiles } from "@/git/queries/status";
import { useUnsuspendedData } from "@/git/queries/unsuspended";

/**
 * What's under way in the repository, above the history and a file's changes: a merge, a rebase,
 * cherry-picks or reverts, with how many files still have conflicts, and the conflicts a stash left
 * as it was popped. The first conflicted file is a click away; once none is left, the operation is
 * continued, as git would from a terminal. Aborting it, which throws away what's been resolved, asks
 * first.
 */
export function OperationBar(props: {
  repositoryId: string;
  /** Opens a conflicted file's conflicts. */
  onResolve: (file: ChangedFile) => void;
  /** What aborting waits for: the edits to a file on show saved. Resolves to whether to go on. */
  beforeAbort: () => Promise<boolean>;
}) {
  const operation = useUnsuspendedData(useOperationInProgress(() => props.repositoryId));
  const conflicted = useUnsuspendedData(useConflictedFiles(() => props.repositoryId));
  const actions = useOperationActions(() => props.repositoryId);
  const [confirming, setConfirming] = createSignal<Operation>();

  const files = () => conflicted()?.files ?? [];
  const ready = () => conflicted()?.markerFree.length ?? 0;
  /** The conflicted file to open first: one with markers left, if any is. */
  const first = () => {
    const free = new Set(conflicted()?.markerFree);
    return files().find((file) => !free.has(file.path)) ?? files()[0];
  };
  const pending = () => actions.continue.isPending() || actions.abort.isPending();

  const summary = () => {
    const count = files().length;
    if (count === 0) return operation() ? "All conflicts resolved" : "";
    const conflicts = `${count} ${count === 1 ? "file has" : "files have"} conflicts`;
    return ready() > 0 ? `${conflicts}, ${ready()} ready to mark resolved` : conflicts;
  };

  return (
    <Show when={operation() || files().length > 0}>
      <div
        role="status"
        class="flex min-w-0 shrink-0 items-center gap-2 border-b border-border bg-panel px-3 py-1.5 text-[12px]"
      >
        <GitMergeConflict
          size={15}
          class={cn("shrink-0", files().length > 0 ? "text-coral" : "text-mint")}
        />
        <span class="flex min-w-0 items-baseline gap-2">
          <strong class="truncate font-[640]">
            {operation() ? operationTitle(operation()!) : "Conflicts"}
          </strong>
          <Show when={operation() && operationProgress(operation()!)}>
            {(progress) => <span class="shrink-0 text-[11px] text-faint">{progress()}</span>}
          </Show>
          <span class="shrink-[2] truncate text-[11.5px] text-muted">
            {summary()}
            {!operation() && files().length > 0 ? ". Resolve them, then commit." : ""}
          </span>
        </span>
        <span class="ml-auto flex shrink-0 items-center gap-1.5">
          <Show when={first()}>
            {(file) => (
              <Button
                variant="ghost"
                class="h-[26px] rounded-md px-2 text-[11.5px] font-[600] enabled:hover:translate-y-0"
                onClick={() => props.onResolve(file())}
              >
                Resolve conflicts
              </Button>
            )}
          </Show>
          <Show when={operation()}>
            {(current) => (
              <>
                <FailurePopover
                  title={`Abort ${operationName(current())}`}
                  error={actions.abort.error()}
                  onDismiss={() => actions.abort.dismiss()}
                >
                  <Button
                    disabled={pending()}
                    class="h-[26px] rounded-md px-2 text-[11.5px] font-[600] enabled:hover:translate-y-0"
                    onClick={() => setConfirming(current())}
                  >
                    Abort
                  </Button>
                </FailurePopover>
                <FailurePopover
                  title={`Continue ${operationName(current())}`}
                  error={actions.continue.error()}
                  onDismiss={() => actions.continue.dismiss()}
                >
                  <Button
                    variant="primary"
                    disabled={pending() || files().length > 0}
                    class="h-[26px] rounded-md px-2 text-[11.5px] font-[600] shadow-none enabled:hover:translate-y-0"
                    onClick={() => actions.continue.run(current().kind)}
                  >
                    {actions.continue.isPending() ? "Continuing…" : continueLabel(current())}
                  </Button>
                </FailurePopover>
              </>
            )}
          </Show>
        </span>
      </div>
      <AbortDialog
        operation={confirming()}
        onCancel={() => setConfirming(undefined)}
        onAbort={async (current) => {
          setConfirming(undefined);
          if (await props.beforeAbort()) actions.abort.run(current.kind);
        }}
      />
    </Show>
  );
}

/** What continuing does, on its button: commits a merge, goes on with the rest. */
function continueLabel(operation: Operation): string {
  return operation.kind === "merge" ? "Commit merge" : "Continue";
}

/**
 * Asks before aborting an operation, which puts the repository back as it was before it: what's
 * been resolved so far is lost. Esc, or clicking outside, cancels.
 */
function AbortDialog(props: {
  operation: Operation | undefined;
  onCancel: () => void;
  onAbort: (operation: Operation) => void;
}) {
  // The last one asked about, still named while the dialog closes.
  const operation = createMemo((last: Operation | undefined) => props.operation ?? last);
  return (
    <AlertDialog
      open={props.operation !== undefined}
      onOpenChange={(open) => !open && props.onCancel()}
      modal
      preventScroll
    >
      <DialogPortal>
        <AlertDialog.Content class={cn(DIALOG_BOX, "max-w-[440px] p-5")}>
          <div class="flex items-start gap-3">
            <span class="grid size-9 shrink-0 place-items-center rounded-[9px] bg-coral-soft text-coral">
              <TriangleAlert size={18} strokeWidth={1.9} />
            </span>
            <div class="min-w-0">
              <AlertDialog.Title class="m-0 text-[15px] font-[680]">
                Abort {operation() && operationName(operation()!)}?
              </AlertDialog.Title>
              <AlertDialog.Description class="m-0 mt-2 text-[12.5px] leading-[1.55] text-text-soft">
                This puts the repository back as it was before{" "}
                {operation() && operationName(operation()!)} started. The conflicts you've resolved
                so far are lost.
              </AlertDialog.Description>
            </div>
          </div>
          <div class="mt-5 flex justify-end gap-2">
            <Button variant="ghost" onClick={props.onCancel}>
              Cancel
            </Button>
            <Button
              variant="primary"
              onClick={() => props.operation && props.onAbort(props.operation)}
            >
              Abort
            </Button>
          </div>
        </AlertDialog.Content>
      </DialogPortal>
    </AlertDialog>
  );
}
