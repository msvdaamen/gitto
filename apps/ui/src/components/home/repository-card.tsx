import { Link } from "@tanstack/solid-router";
import Check from "lucide-solid/icons/check";
import ChevronRight from "lucide-solid/icons/chevron-right";
import FolderGit2 from "lucide-solid/icons/folder-git-2";
import GitBranch from "lucide-solid/icons/git-branch";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import { Show } from "solid-js";

import { Badge } from "@/components/ui/badge";
import type { Tone } from "@/components/ui/tone";
import { remoteProvider } from "@/git/remote";
import { headLabel, syncLabel } from "@/git/status";
import { useRelativeTime } from "@/hooks/relative-time";

import type { RepositorySummary } from "./summaries";

const providerTone: Record<string, Tone> = {
  GitHub: "purple",
  GitLab: "blue",
};

/** A repository in the home page's list: where it's hosted, its branch, and what's changed. */
export function RepositoryCard(props: { summary: RepositorySummary }) {
  const relativeTime = useRelativeTime();
  const remote = () => props.summary.overview?.remote;
  const provider = () => {
    const url = remote()?.url;
    return url ? remoteProvider(url) : "Local";
  };

  return (
    <Link
      to="/$repoId"
      params={{ repoId: props.summary.repository.id }}
      class="group grid min-h-20.5 w-full cursor-pointer grid-cols-[42px_minmax(0,1fr)_auto_24px] items-center gap-3 rounded-[9px] border border-transparent bg-transparent py-2.75 pr-2.75 pl-3 text-left text-text [-webkit-tap-highlight-color:transparent] hover:border-border hover:bg-panel-hover focus-ring max-sm:grid-cols-[38px_minmax(0,1fr)_18px]"
      data-repository={props.summary.repository.id}
    >
      <span class="grid size-10 place-items-center rounded-[10px] bg-[linear-gradient(145deg,var(--primary-soft),var(--blue-soft))] text-primary-strong shadow-[inset_0_0_0_1px_color-mix(in_srgb,var(--primary)_18%,transparent)]">
        <FolderGit2 size={19} />
      </span>
      <span class="flex min-w-0 flex-col gap-1.2">
        <span class="flex items-center gap-2">
          <strong class="truncate text-[14.5px]">{props.summary.repository.name}</strong>
          {/* Once it's known whether there's a remote. */}
          <Show when={props.summary.overview}>
            <span title={remote()?.url}>
              <Badge tone={providerTone[provider()] ?? "neutral"}>{provider()}</Badge>
            </span>
          </Show>
        </span>
        <span class="truncate text-[13px] text-muted" title={props.summary.repository.path}>
          {props.summary.repository.path}
        </span>
        <span class="flex min-h-4 items-center gap-3.25 text-[12px] text-muted [&>span]:flex [&>span]:items-center [&>span]:gap-1.25 [&>span]:whitespace-nowrap">
          <Show
            when={!props.summary.error}
            fallback={
              <span class="min-w-0 text-coral">
                <TriangleAlert size={12} class="shrink-0" />
                <span class="truncate">{props.summary.error?.message}</span>
              </span>
            }
          >
            <RepositoryStatus summary={props.summary} />
          </Show>
        </span>
      </span>
      <span
        class="self-start pt-1 text-[12px] whitespace-nowrap text-faint max-sm:hidden"
        title="Last commit"
      >
        {props.summary.overview?.committedAt != null &&
          relativeTime(props.summary.overview.committedAt)}
      </span>
      <span class="grid -translate-x-0.75 place-items-center text-faint opacity-0 transition-[opacity,transform] duration-150 group-hover:translate-x-0 group-hover:opacity-100 motion-reduce:transition-none">
        <ChevronRight size={16} />
      </span>
    </Link>
  );
}

/** The branch, how it compares to its upstream, and how many files changed. */
function RepositoryStatus(props: { summary: RepositorySummary }) {
  return (
    <Show when={props.summary.status}>
      {(status) => {
        const files = () => status().counts.files;
        const conflicted = () => status().counts.conflicted;
        return (
          <>
            <span>
              <GitBranch size={13} />
              {headLabel(status().head)}
            </span>
            <Show when={status().upstream && (status().ahead || status().behind)}>
              <span class="text-blue" title={`Compared with ${status().upstream}, as last fetched`}>
                {syncLabel(status())}
              </span>
            </Show>
            <Show when={conflicted()}>
              <span class="text-coral">
                {conflicted()} {conflicted() === 1 ? "conflict" : "conflicts"}
              </span>
            </Show>
            <Show
              when={files() > 0}
              fallback={
                <span class="text-mint">
                  <Check size={12} />
                  Clean
                </span>
              }
            >
              <span class="text-amber">
                {files()} {files() === 1 ? "change" : "changes"}
              </span>
            </Show>
          </>
        );
      }}
    </Show>
  );
}
