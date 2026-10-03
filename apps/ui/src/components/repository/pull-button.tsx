import { Popover } from "@kobalte/core/popover";
import Download from "lucide-solid/icons/download";
import X from "lucide-solid/icons/x";
import { Suspense } from "solid-js";

import { usePull } from "@/git/queries/remote";
import { useStatus } from "@/git/queries/status";
import { headPullBlocker, pullTitle } from "@/git/status";

import { ToolbarButton } from "./toolbar-button";

/** Pulls the current branch's upstream, and says why if that failed or was left to finish. */
export function PullButton(props: { repositoryId: string }) {
  return (
    <Suspense fallback={<ToolbarButton icon={Download} label="Pull" accent disabled />}>
      <Pull repositoryId={props.repositoryId} />
    </Suspense>
  );
}

function Pull(props: { repositoryId: string }) {
  const status = useStatus(() => props.repositoryId);
  const pull = usePull(() => props.repositoryId);

  return (
    <Popover
      open={!!pull.error()}
      onOpenChange={(open) => !open && pull.dismiss()}
      placement="bottom-start"
      gutter={6}
    >
      <Popover.Anchor class="flex">
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
      </Popover.Anchor>
      <Popover.Portal>
        <Popover.Content
          // Not focused: the pull can fail while the user is typing elsewhere.
          onOpenAutoFocus={(event) => event.preventDefault()}
          class="z-50 flex max-w-[360px] animate-toast-in items-start gap-2 rounded-lg border border-[color-mix(in_srgb,var(--coral)_30%,var(--border))] bg-panel-raised p-3 text-text shadow-app motion-reduce:animate-none"
        >
          <div class="min-w-0">
            <Popover.Title class="m-0 text-[12.5px] font-[680]">Pull</Popover.Title>
            <Popover.Description class="m-0 mt-1 text-[11.5px] break-words whitespace-pre-wrap text-coral">
              {pull.error()?.message}
            </Popover.Description>
          </div>
          <Popover.CloseButton
            class="grid size-5 shrink-0 cursor-pointer place-items-center rounded border-0 bg-transparent p-0 text-muted hover:bg-panel-hover hover:text-text focus-ring"
            aria-label="Dismiss"
          >
            <X size={13} />
          </Popover.CloseButton>
        </Popover.Content>
      </Popover.Portal>
    </Popover>
  );
}
