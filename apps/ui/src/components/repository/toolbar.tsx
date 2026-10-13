import { cn } from "cn";
import GitBranch from "lucide-solid/icons/git-branch";
import LoaderCircle from "lucide-solid/icons/loader-circle";
import PanelLeft from "lucide-solid/icons/panel-left";
import PanelRight from "lucide-solid/icons/panel-right";
import Redo2 from "lucide-solid/icons/redo-2";
import RefreshCw from "lucide-solid/icons/refresh-cw";
import Settings from "lucide-solid/icons/settings";
import Undo2 from "lucide-solid/icons/undo-2";
import { createEffect, createSignal, on, Suspense } from "solid-js";

import { IconButton } from "@/components/ui/button";
import { Divider } from "@/components/ui/divider";
import { FailurePopover } from "@/components/ui/failure-popover";
import { TextInput } from "@/components/ui/text-input";
import {
  useDeleteBranchState,
  useMergeBranchState,
  useSwitchBranchState,
} from "@/git/queries/branch";
import { useFetch } from "@/git/queries/remote";
import { useStatus } from "@/git/queries/status";
import { headLabel } from "@/git/status";
import { useRepository } from "@/hooks/repositories";

import { BranchButton } from "./branch-button";
import { PullButton } from "./pull-button";
import { PushButton } from "./push-button";
import { StashButtons } from "./stash-buttons";
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
        <CurrentBranch repositoryId={props.repositoryId} />
      </div>
      <div class="flex items-center gap-0.5">
        <ToolbarButton icon={Undo2} label="Undo" hideBelow="lg" />
        <ToolbarButton icon={Redo2} label="Redo" hideBelow="lg" />
        <Divider class="h-6" />
        <PullButton repositoryId={props.repositoryId} />
        <PushButton repositoryId={props.repositoryId} />
        <BranchButton repositoryId={props.repositoryId} />
        <StashButtons repositoryId={props.repositoryId} />
        <FetchButton repositoryId={props.repositoryId} />
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

/** Fetches every remote; spins while it runs, and turns red with the reason when it fails. */
function FetchButton(props: { repositoryId: string }) {
  const fetchRemotes = useFetch();
  // The last fetch, if it was of this repository; one of another stays with that repository.
  const current = () => fetchRemotes.variables === props.repositoryId;
  const pending = () => current() && fetchRemotes.isPending;
  const error = () => (current() ? fetchRemotes.error : null);

  return (
    <ToolbarButton
      icon={RefreshCw}
      label="Fetch"
      hideBelow="sm"
      title={error() ? `Fetch failed: ${error()!.message}` : "Fetch all remotes"}
      disabled={pending()}
      class={cn(error() && "text-coral")}
      busy={pending()}
      onClick={() => fetchRemotes.mutate(props.repositoryId)}
    />
  );
}

/**
 * The repository's name and current branch. A spinner takes the branch icon's place while switching
 * branches, merging one into it or deleting one (from a branch's menu), and a popover under it says
 * why that failed.
 */
function CurrentBranch(props: { repositoryId: string }) {
  const switching = useSwitchBranchState(() => props.repositoryId);
  const merging = useMergeBranchState(() => props.repositoryId);
  const deleting = useDeleteBranchState(() => props.repositoryId);
  const busy = () => switching.isPending() || merging.isPending() || deleting.isPending();
  const failures = {
    merge: { title: "Merge", error: merging.error, dismiss: () => merging.dismiss() },
    switch: { title: "Switch branch", error: switching.error, dismiss: () => switching.dismiss() },
    delete: { title: "Delete branch", error: deleting.error, dismiss: () => deleting.dismiss() },
  };
  type Failed = keyof typeof failures;
  // Which failed, the last first: each can have failed, one after the other, and they're shown
  // one at a time.
  const [order, setOrder] = createSignal<Failed[]>(["merge", "switch", "delete"]);
  const failed = (name: Failed) => (error: Error | null) =>
    error && setOrder((names) => [name, ...names.filter((other) => other !== name)]);
  createEffect(on(merging.error, failed("merge")));
  createEffect(on(switching.error, failed("switch")));
  createEffect(on(deleting.error, failed("delete")));
  const failure = () => {
    const shown = failures[order().find((name) => failures[name].error()) ?? "merge"];
    return { title: shown.title, error: shown.error(), dismiss: shown.dismiss };
  };

  return (
    <FailurePopover
      title={failure().title}
      error={failure().error}
      onDismiss={() => failure().dismiss()}
      class="min-w-0"
    >
      <div class="flex min-w-0 items-center gap-2" aria-busy={busy()}>
        <span class="grid size-[27px] shrink-0 place-items-center rounded-[7px] bg-primary-soft text-primary-strong">
          {busy() ? (
            <LoaderCircle size={15} class="animate-spin motion-reduce:animate-none" />
          ) : (
            <GitBranch size={15} />
          )}
        </span>
        <div class="flex min-w-0 flex-col gap-px max-md:hidden">
          <Suspense>
            <RepositoryName repositoryId={props.repositoryId} />
          </Suspense>
        </div>
      </div>
    </FailurePopover>
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
