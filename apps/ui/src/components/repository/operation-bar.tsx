import type { ChangedFile, Operation } from "@gitto/git/types";
import { cn } from "cn";
import GitMergeConflict from "lucide-solid/icons/git-merge-conflict";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import { createSignal, Show } from "solid-js";

import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { FailurePopover } from "@/components/ui/failure-popover";
import { operationName, operationProgress, operationTitle } from "@/git/conflicts";
import { useOperationActions, useOperationInProgress } from "@/git/queries/progress";
import { firstToResolve, useConflictedFiles } from "@/git/queries/status";
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
  /**
   * What continuing or aborting waits for, as either rewrites files in the working tree: the
   * edits to a file on show saved. Resolves to whether to go on.
   */
  beforeChange: () => Promise<boolean>;
}) {
  const operation = useUnsuspendedData(useOperationInProgress(() => props.repositoryId));
  const conflicted = useUnsuspendedData(useConflictedFiles(() => props.repositoryId));
  const actions = useOperationActions(() => props.repositoryId);
  const [confirming, setConfirming] = createSignal<Operation>();

  const files = () => conflicted()?.files ?? [];
  const ready = () => conflicted()?.markerFree.length ?? 0;
  const first = () => {
    const current = conflicted();
    return current && firstToResolve(current);
  };
  const pending = () => actions.continue.isPending() || actions.abort.isPending();
  /** Continues `current` once the edits to a file on show are saved: it rewrites files. */
  const goOn = async (current: Operation) => {
    if (await props.beforeChange()) actions.continue.run(current.kind);
  };
  /** Aborts `current` once the edits to a file on show are saved, as continuing does. */
  const abort = async (current: Operation) => {
    if (await props.beforeChange()) actions.abort.run(current.kind);
  };

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
                    onClick={() => void goOn(current())}
                  >
                    {actions.continue.isPending() ? "Continuing…" : continueLabel(current())}
                  </Button>
                </FailurePopover>
              </>
            )}
          </Show>
        </span>
      </div>
      {/* Aborting puts the repository back as it was before the operation: what's been resolved
          so far is lost. */}
      <ConfirmDialog
        item={confirming()}
        icon={TriangleAlert}
        title={(current) => `Abort ${operationName(current)}?`}
        description={(current) =>
          `This puts the repository back as it was before ${operationName(current)} started. The conflicts you've resolved so far are lost.`
        }
        confirmLabel="Abort"
        onCancel={() => setConfirming(undefined)}
        onConfirm={(current) => {
          setConfirming(undefined);
          void abort(current);
        }}
      />
    </Show>
  );
}

/** What continuing does, on its button: commits a merge, goes on with the rest. */
function continueLabel(operation: Operation): string {
  return operation.kind === "merge" ? "Commit merge" : "Continue";
}
