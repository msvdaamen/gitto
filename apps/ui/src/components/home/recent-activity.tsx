import type { ActivityKind } from "@gitto/git/types";
import { Link } from "@tanstack/solid-router";
import { cn } from "cn";
import type { LucideIcon } from "lucide-solid";
import ArrowDownToLine from "lucide-solid/icons/arrow-down-to-line";
import Cherry from "lucide-solid/icons/cherry";
import Clock from "lucide-solid/icons/clock";
import FolderDown from "lucide-solid/icons/folder-down";
import GitBranch from "lucide-solid/icons/git-branch";
import GitCommitHorizontal from "lucide-solid/icons/git-commit-horizontal";
import GitMerge from "lucide-solid/icons/git-merge";
import PencilLine from "lucide-solid/icons/pencil-line";
import RotateCcw from "lucide-solid/icons/rotate-ccw";
import RotateCcwClock from "lucide-solid/icons/rotate-ccw-clock";
import Undo2 from "lucide-solid/icons/undo-2";
import { createMemo, For, Show } from "solid-js";
import { Dynamic } from "solid-js/web";

import { Card } from "@/components/ui/card";
import { toneClasses, type Tone } from "@/components/ui/tone";
import { describeActivity } from "@/git/activity";
import { useRelativeTime } from "@/hooks/relative-time";

import type { RepositorySummary } from "./summaries";

/** How many of the latest things done, across all repositories, are shown. */
const SHOWN = 6;

const activityStyles: Record<ActivityKind, { icon: LucideIcon; tone: Tone }> = {
  commit: { icon: GitCommitHorizontal, tone: "purple" },
  amend: { icon: PencilLine, tone: "purple" },
  merge: { icon: GitMerge, tone: "mint" },
  pull: { icon: ArrowDownToLine, tone: "blue" },
  rebase: { icon: GitBranch, tone: "mint" },
  checkout: { icon: GitBranch, tone: "blue" },
  reset: { icon: Undo2, tone: "amber" },
  "cherry-pick": { icon: Cherry, tone: "coral" },
  revert: { icon: RotateCcw, tone: "amber" },
  clone: { icon: FolderDown, tone: "blue" },
  other: { icon: Clock, tone: "neutral" },
};

/** What was last done in any of the repositories, as their reflogs have it. */
export function RecentActivity(props: { summaries: RepositorySummary[] }) {
  const relativeTime = useRelativeTime();
  const entries = createMemo(() =>
    props.summaries
      .flatMap(({ repository, overview }) =>
        (overview?.activity ?? []).map((activity) => ({ repository, activity })),
      )
      .toSorted((a, b) => b.activity.at - a.activity.at)
      .slice(0, SHOWN),
  );

  return (
    <Card icon={RotateCcwClock} title="Recent activity">
      <Show
        when={entries().length}
        fallback={
          <p class="m-0 px-1.25 py-2 text-[12px] text-faint">
            Commits, merges and checkouts in your repositories show here.
          </p>
        }
      >
        <div class="flex flex-col">
          <For each={entries()}>
            {({ repository, activity }) => {
              const { title, action } = describeActivity(activity);
              const style = activityStyles[activity.kind];
              return (
                <Link
                  to="/$repoId"
                  params={{ repoId: repository.id }}
                  class="grid grid-cols-[27px_minmax(0,1fr)_auto] items-center gap-2 rounded-[7px] border-t border-border-soft px-1.25 py-2 first:border-t-0 hover:bg-panel-hover focus-ring"
                  title={activity.message}
                >
                  <span
                    class={cn(
                      "grid size-6.25 place-items-center rounded-[7px]",
                      toneClasses[style.tone],
                    )}
                  >
                    <Dynamic component={style.icon} size={13} />
                  </span>
                  <div class="flex min-w-0 flex-col gap-0.5">
                    <strong class="truncate text-[12px]">{title}</strong>
                    <span class="truncate text-[11px] text-faint">
                      {repository.name}
                      {action && ` · ${action}`}
                    </span>
                  </div>
                  <time
                    class="text-[11px] whitespace-nowrap text-faint"
                    dateTime={new Date(activity.at).toISOString()}
                  >
                    {relativeTime(activity.at)}
                  </time>
                </Link>
              );
            }}
          </For>
        </div>
      </Show>
    </Card>
  );
}
