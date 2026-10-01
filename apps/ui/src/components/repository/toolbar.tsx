import { cn } from "cn";
import type { LucideIcon } from "lucide-solid";
import Archive from "lucide-solid/icons/archive";
import ArchiveRestore from "lucide-solid/icons/archive-restore";
import Download from "lucide-solid/icons/download";
import GitBranch from "lucide-solid/icons/git-branch";
import PanelLeft from "lucide-solid/icons/panel-left";
import PanelRight from "lucide-solid/icons/panel-right";
import Redo2 from "lucide-solid/icons/redo-2";
import RefreshCw from "lucide-solid/icons/refresh-cw";
import Settings from "lucide-solid/icons/settings";
import Undo2 from "lucide-solid/icons/undo-2";
import Upload from "lucide-solid/icons/upload";
import { For, Suspense } from "solid-js";
import { Dynamic } from "solid-js/web";

import { IconButton } from "@/components/ui/button";
import { Divider } from "@/components/ui/divider";
import { TextInput } from "@/components/ui/text-input";
import { headLabel, useStatus } from "@/git/status";
import { useRepository } from "@/hooks/repository";

const toolbarActions: { icon: LucideIcon; label: string; accent?: boolean }[] = [
  { icon: Undo2, label: "Undo" },
  { icon: Redo2, label: "Redo" },
  { icon: Download, label: "Pull", accent: true },
  { icon: Upload, label: "Push" },
  { icon: GitBranch, label: "Branch" },
  { icon: Archive, label: "Stash" },
  { icon: ArchiveRestore, label: "Pop" },
  { icon: RefreshCw, label: "Fetch" },
];

export function RepositoryToolbar(props: {
  repositoryId: string;
  search: string;
  onSearch: (value: string) => void;
  sidebarOpen: boolean;
  onToggleSidebar: () => void;
  detailsOpen: boolean;
  onToggleDetails: () => void;
}) {
  return (
    <header class="flex min-w-0 items-center gap-[7px] border-b border-border bg-panel px-[9px] max-sm:gap-[3px]">
      <div class="flex min-w-[195px] items-center gap-[7px] max-md:min-w-fit">
        <IconButton
          label="Toggle sidebar"
          icon={PanelLeft}
          active={props.sidebarOpen}
          onClick={props.onToggleSidebar}
        />
        <div class="flex min-w-0 items-center gap-2">
          <span class="grid size-[27px] shrink-0 place-items-center rounded-[7px] bg-primary-soft text-primary-strong">
            <GitBranch size={15} />
          </span>
          <div class="flex min-w-0 flex-col gap-px max-md:hidden">
            <Suspense>
              <RepositoryName repositoryId={props.repositoryId} />
            </Suspense>
          </div>
        </div>
      </div>
      <div class="flex items-center gap-0.5">
        <For each={toolbarActions}>
          {(action, index) => (
            <>
              {index() === 2 && <Divider class="h-6" />}
              <button
                class={cn(
                  "flex h-[38px] min-w-[43px] cursor-pointer flex-col items-center justify-center gap-0.5 rounded-md border-0 bg-transparent px-1.5 text-muted hover:bg-panel-hover hover:text-text max-md:min-w-9 max-md:[&>span]:hidden",
                  action.accent && "text-blue",
                  [0, 1, 5, 6].includes(index()) && "max-lg:hidden",
                  index() >= 4 && "max-sm:hidden",
                )}
                title={action.label}
              >
                <Dynamic component={action.icon} size={16} />
                <span class="text-[8px]">{action.label}</span>
              </button>
            </>
          )}
        </For>
      </div>
      <div class="ml-auto max-md:min-w-[115px] max-sm:min-w-[90px]">
        <TextInput
          compact
          value={props.search}
          onChange={props.onSearch}
          placeholder="Search history…"
        />
      </div>
      <IconButton
        label="Toggle details panel"
        icon={PanelRight}
        active={props.detailsOpen}
        onClick={props.onToggleDetails}
      />
      <IconButton label="Repository settings" icon={Settings} />
    </header>
  );
}

function RepositoryName(props: { repositoryId: string }) {
  const repository = useRepository(() => props.repositoryId);
  const status = useStatus(() => props.repositoryId);

  return (
    <>
      <strong class="truncate text-[11px]">{repository()?.name}</strong>
      <span class="truncate text-[8.5px] text-faint">
        {status.data ? headLabel(status.data.head) : ""}
      </span>
    </>
  );
}
