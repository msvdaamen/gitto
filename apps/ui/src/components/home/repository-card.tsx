import Check from "lucide-solid/icons/check";
import ChevronRight from "lucide-solid/icons/chevron-right";
import Folder from "lucide-solid/icons/folder";
import GitBranch from "lucide-solid/icons/git-branch";
import Pin from "lucide-solid/icons/pin";
import { Show } from "solid-js";

import { Badge } from "@/components/ui/badge";
import { StatusDot } from "@/components/ui/status-dot";
import type { Tone } from "@/components/ui/tone";
import type { RepositorySummary } from "@/types/git";

const providerTone: Record<RepositorySummary["provider"], Tone> = {
  GitHub: "purple",
  GitLab: "blue",
  Local: "neutral",
};

/** A repository in the recent repositories list: name, branch, language and pending changes. */
export function RepositoryCard(props: { repository: RepositorySummary; onOpen: () => void }) {
  const changeCount = () => {
    const { modified, added, deleted } = props.repository.status;
    return modified + added + deleted;
  };

  return (
    <button
      class="group grid min-h-20.5 w-full cursor-pointer grid-cols-[42px_minmax(0,1fr)_auto_24px] items-center gap-3 rounded-[9px] border border-transparent bg-transparent py-2.75 pr-2.75 pl-3 text-left text-text [-webkit-tap-highlight-color:transparent] hover:border-border hover:bg-panel-hover focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary max-[700px]:grid-cols-[38px_minmax(0,1fr)_18px]"
      onClick={props.onOpen}
      data-repository={props.repository.id}
    >
      <span class="grid size-10 place-items-center rounded-[10px] bg-[linear-gradient(145deg,var(--primary-soft),var(--blue-soft))] text-primary-strong shadow-[inset_0_0_0_1px_color-mix(in_srgb,var(--primary)_18%,transparent)]">
        {props.repository.provider === "Local" ? <Folder size={19} /> : <GitBranch size={19} />}
      </span>
      <span class="flex min-w-0 flex-col gap-1.2">
        <span class="flex items-center gap-2">
          <strong class="text-[13px]">{props.repository.name}</strong>
          {props.repository.pinned && <Pin class="text-amber" size={12} />}
          <Badge tone={providerTone[props.repository.provider]}>{props.repository.provider}</Badge>
        </span>
        <span class="truncate text-[10.5px] text-muted">{props.repository.description}</span>
        <span class="flex items-center gap-3.25 text-[9.5px] text-muted [&>span]:flex [&>span]:items-center [&>span]:gap-1.25 [&>span]:whitespace-nowrap">
          <span>
            <GitBranch size={13} />
            {props.repository.branch}
          </span>
          <span>
            <StatusDot color={props.repository.languageColor} />
            {props.repository.language}
          </span>
          <Show
            when={changeCount() > 0}
            fallback={
              <span class="text-mint">
                <Check size={12} />
                Clean
              </span>
            }
          >
            <span class="text-amber">{changeCount()} changes</span>
          </Show>
        </span>
      </span>
      <span class="self-start pt-1 text-[9.5px] whitespace-nowrap text-faint max-[700px]:hidden">
        {props.repository.lastOpened}
      </span>
      <span class="grid -translate-x-0.75 place-items-center text-faint opacity-0 transition-[opacity,transform] duration-150 group-hover:translate-x-0 group-hover:opacity-100 motion-reduce:transition-none">
        <ChevronRight size={16} />
      </span>
    </button>
  );
}
