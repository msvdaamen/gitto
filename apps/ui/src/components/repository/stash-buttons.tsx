import Archive from "lucide-solid/icons/archive";
import ArchiveRestore from "lucide-solid/icons/archive-restore";
import { Suspense } from "solid-js";

import { FailurePopover } from "@/components/ui/failure-popover";
import { useStashActions, useStashes } from "@/git/queries/stash";
import { useStatus } from "@/git/queries/status";
import { canPop, popTitle, stashBlocker, stashTitle } from "@/git/stash";

import { ToolbarButton } from "./toolbar-button";

/**
 * Stashes every change, untracked files included, and pops the newest stash; each says why if it
 * failed. One at a time, along with the stashes popped and deleted from the sidebar: a pop names the
 * stash it pops, which a stash still being made would change.
 */
export function StashButtons(props: { repositoryId: string }) {
  return (
    <Suspense
      fallback={
        <>
          <ToolbarButton icon={Archive} label="Stash" hideBelow="lg" disabled />
          <ToolbarButton icon={ArchiveRestore} label="Pop" hideBelow="lg" disabled />
        </>
      }
    >
      <Stash repositoryId={props.repositoryId} />
    </Suspense>
  );
}

function Stash(props: { repositoryId: string }) {
  const status = useStatus(() => props.repositoryId);
  const stashes = useStashes(() => props.repositoryId);
  const { stash, pop, isPending } = useStashActions(() => props.repositoryId);

  return (
    <>
      <FailurePopover title="Stash" error={stash.error()} onDismiss={() => stash.dismiss()}>
        <ToolbarButton
          icon={Archive}
          label="Stash"
          hideBelow="lg"
          title={status.data ? stashTitle(status.data) : "Stash"}
          disabled={!status.data || !!stashBlocker(status.data) || isPending()}
          busy={stash.isPending()}
          onClick={() => stash.run()}
        />
      </FailurePopover>
      <FailurePopover title="Pop" error={pop.error()} onDismiss={() => pop.dismiss()}>
        <ToolbarButton
          icon={ArchiveRestore}
          label="Pop"
          hideBelow="lg"
          title={
            status.data && stashes.data
              ? popTitle(status.data, stashes.data)
              : stashes.error
                ? `Couldn't load the stashes: ${stashes.error.message}`
                : "Pop"
          }
          disabled={!canPop(status.data, stashes.data) || isPending()}
          busy={pop.isPending()}
          count={stashes.data?.length}
          onClick={() => {
            const newest = stashes.data?.[0];
            if (newest) pop.run(newest.sha);
          }}
        />
      </FailurePopover>
    </>
  );
}
