import { cn } from "cn";
import type { LucideIcon } from "lucide-solid";
import GitBranch from "lucide-solid/icons/git-branch";
import GitMerge from "lucide-solid/icons/git-merge";
import RotateCcwClock from "lucide-solid/icons/rotate-ccw-clock";
import Upload from "lucide-solid/icons/upload";
import { For } from "solid-js";
import { Dynamic } from "solid-js/web";

import { LinkButton } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { toneClasses } from "@/components/ui/tone";
import type { ActivityEntry } from "@/data/mock-data";

const activityIcons: Record<ActivityEntry["kind"], LucideIcon> = {
  push: Upload,
  merge: GitMerge,
  branch: GitBranch,
};

export function RecentActivity(props: { activities: ActivityEntry[] }) {
  return (
    <Card
      icon={RotateCcwClock}
      title="Recent activity"
      action={<LinkButton class="text-[12px]">View all</LinkButton>}
    >
      <div class="flex flex-col">
        <For each={props.activities}>
          {(activity) => (
            <div class="grid grid-cols-[27px_minmax(0,1fr)_auto] items-center gap-2 border-t border-border-soft px-1.25 py-2 first:border-t-0">
              <span
                class={cn(
                  "grid size-6.25 place-items-center rounded-[7px]",
                  toneClasses[activity.tone],
                )}
              >
                <Dynamic component={activityIcons[activity.kind]} size={13} />
              </span>
              <div class="flex min-w-0 flex-col gap-0.5">
                <strong class="truncate text-[12px]">{activity.action}</strong>
                <span class="text-[11px] text-faint">
                  {activity.repository} · {activity.branch}
                </span>
              </div>
              <time class="text-[11px] whitespace-nowrap text-faint">{activity.time}</time>
            </div>
          )}
        </For>
      </div>
    </Card>
  );
}
