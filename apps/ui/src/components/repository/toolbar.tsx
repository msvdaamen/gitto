import Archive from "lucide-solid/icons/archive";
import ArchiveRestore from "lucide-solid/icons/archive-restore";
import GitBranch from "lucide-solid/icons/git-branch";
import PanelLeft from "lucide-solid/icons/panel-left";
import PanelRight from "lucide-solid/icons/panel-right";
import Redo2 from "lucide-solid/icons/redo-2";
import RefreshCw from "lucide-solid/icons/refresh-cw";
import Settings from "lucide-solid/icons/settings";
import Undo2 from "lucide-solid/icons/undo-2";
import Upload from "lucide-solid/icons/upload";
import { Suspense } from "solid-js";

import { IconButton } from "@/components/ui/button";
import { Divider } from "@/components/ui/divider";
import { TextInput } from "@/components/ui/text-input";
import { useStatus } from "@/git/queries/status";
import { headLabel } from "@/git/status";
import { useRepository } from "@/hooks/repositories";

import { PullButton } from "./pull-button";
import { ToolbarButton } from "./toolbar-button";

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
        <ToolbarButton icon={Undo2} label="Undo" hideBelow="lg" />
        <ToolbarButton icon={Redo2} label="Redo" hideBelow="lg" />
        <Divider class="h-6" />
        <PullButton repositoryId={props.repositoryId} />
        <ToolbarButton icon={Upload} label="Push" />
        <ToolbarButton icon={GitBranch} label="Branch" hideBelow="sm" />
        <ToolbarButton icon={Archive} label="Stash" hideBelow="lg" />
        <ToolbarButton icon={ArchiveRestore} label="Pop" hideBelow="lg" />
        <ToolbarButton icon={RefreshCw} label="Fetch" hideBelow="sm" />
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
      <strong class="truncate text-[13.5px]">{repository()?.name}</strong>
      <span class="truncate text-[11px] text-faint">
        {status.data ? headLabel(status.data.head) : ""}
      </span>
    </>
  );
}
