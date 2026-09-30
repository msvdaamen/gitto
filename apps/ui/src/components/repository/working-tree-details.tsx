import type { ChangedFile } from "@gitto/git/types";
import type { LucideIcon } from "lucide-solid";
import CircleCheck from "lucide-solid/icons/circle-check";
import FilePen from "lucide-solid/icons/file-pen";
import Minus from "lucide-solid/icons/minus";
import Plus from "lucide-solid/icons/plus";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import { Show, type JSX } from "solid-js";
import { Dynamic } from "solid-js/web";

import { Badge } from "@/components/ui/badge";
import { EmptyState } from "@/components/ui/empty-state";
import { useWorkingTreeChanges } from "@/git/diff";
import { stagingPaths, useStage, useUnstage } from "@/git/staging";
import { useStatus } from "@/git/status";

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

  return (
    <div class="flex min-h-0 flex-1 flex-col">
      <div class="flex shrink-0 items-center gap-[13px] border-b border-border p-4">
        <span class="relative inline-flex h-11 w-[52px] shrink-0 items-center justify-center rounded-[51%_49%_43%_57%/57%_42%_58%_43%] bg-[linear-gradient(145deg,#bc8aef,#7861e6)] shadow-[0_8px_20px_rgba(122,76,170,.2)] before:size-1 before:rounded-full before:bg-[#2b2032] before:shadow-[14px_0_#2b2032] before:content-['']">
          <i />
          <i />
        </span>
        <div>
          <Badge tone="amber">WIP</Badge>
          <h2 class="mt-[7px] mb-1.5 text-sm leading-[1.35] tracking-[-.2px]">
            Uncommitted changes
          </h2>
          <p class="m-0 text-[9.5px] leading-[1.55] text-muted">
            On <strong class="text-text-soft">{status.data?.branch ?? "detached HEAD"}</strong>
            {" · "}
            {changes.staged().length} staged, {changes.unstaged().length} unstaged
          </p>
        </div>
      </div>

      <Show when={changes.query.error ?? stage.error ?? unstage.error}>
        {(error) => (
          <EmptyState
            icon={TriangleAlert}
            title="Something went wrong"
            tone="error"
            class="h-auto shrink-0 border-b border-border py-4"
          >
            {error().message}
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
        onBulk={() => stage.mutate(stagingPaths(changes.unstaged()))}
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
        onBulk={() => unstage.mutate(stagingPaths(changes.staged()))}
        action={{
          label: "Unstage",
          icon: Minus,
          disabled: busy(),
          run: (file) => unstage.mutate(stagingPaths([file])),
        }}
      />

      <CommitForm repositoryId={props.repositoryId} stagedCount={changes.staged().length} />
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
  return (
    <section class="flex min-h-[130px] flex-1 basis-0 flex-col border-b border-border pt-3">
      <div class="flex shrink-0 items-center justify-between pt-0 pr-3.5 pb-2 pl-3.5">
        <div class={`flex items-center gap-1.5 ${props.tone === "mint" ? "text-mint" : ""}`}>
          <Dynamic component={props.icon} size={15} />
          <strong class="text-[10px] text-text">{props.title}</strong>
          <Badge tone={props.tone}>{props.files.length}</Badge>
        </div>
        <button
          class="cursor-pointer border-0 bg-transparent text-[9px] text-primary-strong disabled:cursor-default disabled:opacity-50"
          disabled={props.busy || props.files.length === 0}
          onClick={props.onBulk}
        >
          {props.bulkLabel}
        </button>
      </div>
      <div class="min-h-0 flex-1 overflow-y-auto px-2.5 pb-3">
        <Show
          when={props.files.length}
          fallback={<p class="m-0 px-1 pb-1 text-[9px] text-faint">{props.empty}</p>}
        >
          <ChangedFileList files={props.files} action={props.action} />
        </Show>
      </div>
    </section>
  );
}
