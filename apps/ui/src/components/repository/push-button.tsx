import { useQuery } from "@tanstack/solid-query";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import Upload from "lucide-solid/icons/upload";
import { createMemo, Show, Suspense } from "solid-js";

import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { FailurePopover } from "@/components/ui/failure-popover";
import { pushTitle } from "@/git/push";
import { pushRejection, pushTargetQuery, usePull, usePush } from "@/git/queries/remote";
import { useStatus } from "@/git/queries/status";
import { headPullBlocker } from "@/git/status";

import { ToolbarButton } from "./toolbar-button";

/**
 * Pushes the current branch, and says why if that failed. When the remote branch has commits the
 * branch doesn't, asks what to do instead: force-push, or pull, whichever suits.
 */
export function PushButton(props: { repositoryId: string }) {
  return (
    <Suspense fallback={<ToolbarButton icon={Upload} label="Push" disabled />}>
      <Push repositoryId={props.repositoryId} />
    </Suspense>
  );
}

function Push(props: { repositoryId: string }) {
  const target = useQuery(() => pushTargetQuery(props.repositoryId));
  const status = useStatus(() => props.repositoryId);
  const push = usePush(() => props.repositoryId);
  const pull = usePull(() => props.repositoryId);
  // Asked about in the dialog rather than shown as a failure.
  const rejection = () => pushRejection(push.error());
  // Pulling only brings in the remote branch's commits if it's the one pulled from.
  const canPull = () =>
    !!status.data &&
    !headPullBlocker(status.data) &&
    target.data?.kind === "remote" &&
    target.data.pullsFrom;

  return (
    <>
      <FailurePopover
        title="Push"
        error={rejection() ? null : push.error()}
        onDismiss={() => push.dismiss()}
      >
        <ToolbarButton
          icon={Upload}
          label="Push"
          title={
            target.data
              ? pushTitle(target.data)
              : target.error
                ? `Couldn't tell where to push: ${target.error.message}`
                : "Push"
          }
          disabled={target.data?.kind !== "remote" || push.isPending()}
          busy={push.isPending()}
          count={target.data?.kind === "remote" ? target.data.ahead : undefined}
          onClick={() => push.push()}
        />
      </FailurePopover>
      <PushRejectedDialog
        rejection={rejection() ? { kind: rejection()!, message: push.error()!.message } : undefined}
        canPull={canPull()}
        onCancel={() => push.dismiss()}
        onPull={() => {
          push.dismiss();
          pull.pull();
        }}
        onForcePush={() => push.forcePush()}
      />
    </>
  );
}

/** Why the remote branch turned a push down, and how git put it. */
type Rejection = { kind: "rewritten" | "behind"; message: string };

/**
 * Says why the remote branch turned a push down, and offers what to do instead: to force-push when
 * it has the commits from before a rebase or an amend, which pulling would bring back; to pull
 * when it has someone else's, which force-pushing wouldn't replace. Open while there's a
 * `rejection`; Esc, or clicking outside, cancels.
 */
function PushRejectedDialog(props: {
  rejection: Rejection | undefined;
  canPull: boolean;
  onCancel: () => void;
  onPull: () => void;
  onForcePush: () => void;
}) {
  // The last one, still shown while the dialog closes.
  const rejection = createMemo<Rejection>((last) => props.rejection ?? last, {
    kind: "behind",
    message: "",
  });
  const rewritten = () => rejection().kind === "rewritten";
  return (
    <ConfirmDialog
      open={!!props.rejection}
      icon={TriangleAlert}
      tone="amber"
      title="Push rejected"
      description={
        <>
          {rejection().message}{" "}
          {rewritten() ? "Force-push to replace them." : props.canPull && "Pull them first."}
        </>
      }
      onCancel={props.onCancel}
    >
      <Button variant="ghost" onClick={props.onCancel}>
        {rewritten() || props.canPull ? "Cancel" : "Close"}
      </Button>
      <Show when={props.canPull}>
        <Button variant={rewritten() ? "secondary" : "primary"} onClick={props.onPull}>
          Pull
        </Button>
      </Show>
      <Show when={rewritten()}>
        <Button variant="primary" onClick={props.onForcePush}>
          Force push
        </Button>
      </Show>
    </ConfirmDialog>
  );
}
