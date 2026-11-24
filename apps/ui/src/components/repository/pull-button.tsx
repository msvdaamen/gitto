import Download from "@lucide/solid/icons/download";
import { Errored, Loading } from "solid-js";

import { FailurePopover } from "@/components/ui/failure-popover";
import { usePull } from "@/git/queries/remote";
import { useStatus } from "@/git/queries/status";
import { headPullBlocker, pullTitle } from "@/git/status";

import { ToolbarButton } from "./toolbar-button";

/** Pulls the current branch's upstream, and says why if that failed or was left to finish. */
export function PullButton(props: { repositoryId: string }) {
  return (
    <Loading fallback={<ToolbarButton icon={Download} label="Pull" accent disabled />}>
      <Errored
        fallback={(error) => (
          <ToolbarButton
            icon={Download}
            label="Pull"
            accent
            title={`Couldn't load the repository: ${(error() as Error).message}`}
            disabled
          />
        )}
      >
        <Pull repositoryId={props.repositoryId} />
      </Errored>
    </Loading>
  );
}

function Pull(props: { repositoryId: string }) {
  const status = useStatus(() => props.repositoryId);
  const pull = usePull(() => props.repositoryId);

  return (
    <FailurePopover title="Pull" error={pull.error()} onDismiss={() => pull.dismiss()}>
      <ToolbarButton
        icon={Download}
        label="Pull"
        accent
        title={status.data ? pullTitle(status.data) : "Pull"}
        disabled={!status.data || !!headPullBlocker(status.data) || pull.isPending()}
        busy={pull.isPending()}
        count={status.data?.behind}
        onClick={() => pull.pull()}
      />
    </FailurePopover>
  );
}
