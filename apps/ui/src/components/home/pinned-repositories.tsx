import ChevronRight from "lucide-solid/icons/chevron-right";
import Ellipsis from "lucide-solid/icons/ellipsis";
import Pin from "lucide-solid/icons/pin";
import { For } from "solid-js";

import { IconButton } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import type { RepositorySummary } from "@/data/mock-data";

export function PinnedRepositories(props: {
  repositories: RepositorySummary[];
  onOpenRepository: () => void;
}) {
  return (
    <Card
      icon={Pin}
      title="Pinned"
      action={<IconButton label="Manage pinned repositories" icon={Ellipsis} />}
    >
      <For each={props.repositories}>
        {(repository) => (
          <button
            class="grid w-full cursor-pointer grid-cols-[8px_minmax(0,1fr)_auto] items-center gap-2.25 rounded-[7px] border-0 bg-transparent p-2 text-left hover:bg-panel-hover [&>svg]:text-faint"
            onClick={props.onOpenRepository}
          >
            <span class="size-1.5 rounded-full bg-primary shadow-[0_0_8px_color-mix(in_srgb,var(--primary)_55%,transparent)]" />
            <span class="flex min-w-0 flex-col gap-0.5">
              <strong class="text-[13px]">{repository.name}</strong>
              <small class="text-[11.5px] text-faint">{repository.owner}</small>
            </span>
            <ChevronRight size={14} />
          </button>
        )}
      </For>
    </Card>
  );
}
