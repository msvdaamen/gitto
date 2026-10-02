import type { ChangedFile } from "@gitto/git/types";
import type { LucideIcon } from "lucide-solid";
import CircleCheck from "lucide-solid/icons/circle-check";
import FilePen from "lucide-solid/icons/file-pen";
import Minus from "lucide-solid/icons/minus";
import Plus from "lucide-solid/icons/plus";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import { createSignal, Show, type JSX } from "solid-js";

import { Badge } from "@/components/ui/badge";
import { LinkButton } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { Mascot } from "@/components/ui/mascot";
import { SectionHeader } from "@/components/ui/section-header";
import { UpdatingIndicator } from "@/components/ui/updating-indicator";
import { stagingPaths } from "@/git/changes";
import { useWorkingTreeChanges } from "@/git/queries/diff";
import { useStage, useUnstage } from "@/git/queries/staging";
import { useHeadSha, useStatus } from "@/git/queries/status";
import { headLabel } from "@/git/status";
import { useDelayed } from "@/hooks/delayed";

import { ChangedFileList, type FileAction } from "./changed-file-list";
import { CommitForm } from "./commit-form";

/**
 * The uncommitted changes: what's staged for the next commit, what isn't, and the commit form. The
 * two file lists split the space evenly and scroll on their own, so moving files between them
 * doesn't shift the layout.
 */
export function WorkingTreeDetails(props: { repositoryId: string }) {
  const changes = useWorkingTreeChanges(() => props.repositoryId);
  const status = useStatus(() => props.repositoryId);
  const stage = useStage(() => props.repositoryId);
  const unstage = useUnstage(() => props.repositoryId);
  const busy = () => stage.isPending || unstage.isPending;
  const lastCommit = useHeadSha(status);
  // The status is reloaded whenever a file changes, which takes a while in a big repository.
  const updating = useDelayed(() => status.isRefetching);

  return (
    <div class="flex min-h-0 flex-1 flex-col">
      <div class="flex shrink-0 items-center gap-[13px] border-b border-border p-4">
        <Mascot size={52} class="shadow-[0_8px_20px_rgba(122,76,170,.2)]" />
        <div>
          <Badge tone="amber">WIP</Badge>
          <h2 class="mt-[7px] mb-1.5 text-sm leading-[1.35] tracking-[-.2px]">
            Uncommitted changes
          </h2>
          <p class="m-0 text-[12px] leading-[1.55] text-muted">
            On <strong class="text-text-soft">{status.data && headLabel(status.data.head)}</strong>
            {" · "}
            {changes.staged().length} staged, {changes.unstaged().length} unstaged
          </p>
          <Show when={updating()}>
            <UpdatingIndicator class="mt-1" />
          </Show>
        </div>
      </div>

      <Show when={changes.query.error ?? stage.error ?? unstage.error} keyed>
        {(error) => (
          <EmptyState
            icon={TriangleAlert}
            title="Something went wrong"
            tone="error"
            class="h-auto shrink-0 border-b border-border py-4"
          >
            {error.message}
          </EmptyState>
        )}
      </Show>

      <FileSection
        title="Unstaged changes"
        icon={FilePen}
        files={changes.unstaged()}
        empty="Nothing left to stage."
        bulkLabel="Stage all"
        busy={busy()}
        onBulk={() => stage.mutate("all")}
        action={{
          label: "Stage",
          icon: Plus,
          disabled: busy(),
          run: (file) => stage.mutate(stagingPaths([file])),
        }}
      />
      <FileSection
        title="Staged changes"
        icon={CircleCheck}
        tone="mint"
        files={changes.staged()}
        empty="Stage files to include them in the next commit."
        bulkLabel="Unstage all"
        busy={busy()}
        onBulk={() => unstage.mutate("all")}
        action={{
          label: "Unstage",
          icon: Minus,
          disabled: busy(),
          run: (file) => unstage.mutate(stagingPaths([file])),
        }}
      />

      <CommitForm
        repositoryId={props.repositoryId}
        stagedCount={changes.staged().length}
        lastCommit={lastCommit()}
      />
    </div>
  );
}

function FileSection(props: {
  title: string;
  icon: LucideIcon;
  tone?: "mint";
  files: ChangedFile[];
  empty: JSX.Element;
  bulkLabel: string;
  busy: boolean;
  onBulk: () => void;
  action: FileAction;
}) {
  const [scrollElement, setScrollElement] = createSignal<HTMLDivElement>();
  return (
    <section class="flex min-h-[130px] flex-1 basis-0 flex-col border-b border-border pt-3">
      <SectionHeader
        icon={props.icon}
        title={props.title}
        count={props.files.length}
        tone={props.tone}
        class="px-3.5 pb-2"
      >
        <LinkButton disabled={props.busy || props.files.length === 0} onClick={props.onBulk}>
          {props.bulkLabel}
        </LinkButton>
      </SectionHeader>
      <div ref={setScrollElement} class="min-h-0 flex-1 overflow-y-auto px-2.5 pb-3">
        <Show
          when={props.files.length}
          fallback={<p class="m-0 px-1 pb-1 text-[11.5px] text-faint">{props.empty}</p>}
        >
          <ChangedFileList
            files={props.files}
            scrollElement={scrollElement()}
            action={props.action}
          />
        </Show>
      </div>
    </section>
  );
}
