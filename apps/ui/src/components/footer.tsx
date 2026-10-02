import { useParams } from "@tanstack/solid-router";
import Cloud from "lucide-solid/icons/cloud";
import GitBranch from "lucide-solid/icons/git-branch";
import { Show, Suspense } from "solid-js";

import { useStatus } from "@/git/queries/status";
import { headLabel, syncLabel } from "@/git/status";

import { Divider } from "./ui/divider";
import { GittoIcon } from "./ui/gitto-icon";
import { StatusDot } from "./ui/status-dot";

export function Footer() {
  const params = useParams({ strict: false });

  return (
    <footer class="flex items-center justify-between gap-5 border-t border-border bg-[color-mix(in_srgb,var(--bg-soft)_94%,var(--primary)_6%)] px-2.75 text-[13px] text-muted">
      <div class="flex items-center gap-1.25 whitespace-nowrap [&>span]:flex [&>span]:items-center [&>span]:gap-1.25">
        <GittoIcon class="h-3 w-3.5" />
        <StatusDot color="var(--mint)" />
        <span>Ready</span>
        <Show when={params().repoId} keyed>
          {(repositoryId) => (
            <Suspense>
              <RepositoryStatus repositoryId={repositoryId} />
            </Suspense>
          )}
        </Show>
      </div>
      <div class="flex items-center gap-1.25 whitespace-nowrap [&>span]:flex [&>span]:items-center [&>span]:gap-1.25">
        <span>Gitto Preview</span>
        <span class="font-mono text-faint">v0.1.0</span>
      </div>
    </footer>
  );
}

/** The open repository's branch, and how it compares to its upstream. */
function RepositoryStatus(props: { repositoryId: string }) {
  const status = useStatus(() => props.repositoryId);

  return (
    <Show when={status.data} keyed>
      {(data) => (
        <>
          <Divider />
          <span>
            <GitBranch size={12} />
            {headLabel(data.head)}
          </span>
          <Show when={data.upstream} keyed>
            {(upstream) => (
              <span class="text-blue">
                <Cloud size={12} />
                {upstream} · {syncLabel(data)}
              </span>
            )}
          </Show>
        </>
      )}
    </Show>
  );
}
