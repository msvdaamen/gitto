import type { ChangedFile, FileStatus } from "@gitto/git/types";
import Copy from "lucide-solid/icons/copy";
import Ellipsis from "lucide-solid/icons/ellipsis";
import File from "lucide-solid/icons/file";
import GitCommitHorizontal from "lucide-solid/icons/git-commit-horizontal";
import LoaderCircle from "lucide-solid/icons/loader-circle";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import { For, Show, Suspense } from "solid-js";

import { Avatar } from "@/components/ui/avatar";
import { Badge } from "@/components/ui/badge";
import { IconButton } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { useChangedFiles } from "@/git/diff";
import { useHistory } from "@/git/history";
import { useStage } from "@/git/staging";
import { useStatus } from "@/git/status";
import type { Commit } from "@/types/git";

import { CommitForm } from "./commit-form";

const fileStatusLabel: Record<FileStatus, string> = {
  modified: "M",
  added: "A",
  deleted: "D",
  renamed: "R",
  copied: "C",
  typechange: "T",
  untracked: "U",
  conflicted: "!",
};

/** Details of the selected history row: a commit, or the uncommitted changes. */
export function CommitDetails(props: { repositoryId: string; selectedId: string | undefined }) {
  return (
    <Suspense fallback={<EmptyState icon={LoaderCircle} title="Loading details…" />}>
      <SelectedCommit {...props} />
    </Suspense>
  );
}

function SelectedCommit(props: { repositoryId: string; selectedId: string | undefined }) {
  const history = useHistory(
    () => props.repositoryId,
    () => props.selectedId,
  );

  return (
    <Show
      when={history.selected()}
      fallback={<EmptyState icon={GitCommitHorizontal} title="Nothing selected" />}
    >
      {(commit) => <Details commit={commit()} />}
    </Show>
  );
}

function Details(props: { commit: Commit }) {
  const changes = useChangedFiles(() => props.commit);
  const status = useStatus(() => props.commit.repositoryId);
  const stage = useStage(() => props.commit.repositoryId);

  // Renames need their old path staged too, so the deletion side is recorded.
  const stageAll = () =>
    stage.mutate(
      changes.files().flatMap((file) => (file.origPath ? [file.path, file.origPath] : [file.path])),
    );

  return (
    <div class="h-full min-w-[280px] overflow-y-auto">
      <div class="flex h-[38px] items-center justify-between border-b border-border py-0 pr-[9px] pl-[13px] text-[9px] font-[720] tracking-[.07em] text-muted uppercase">
        <span>{props.commit.isWip ? "Working directory" : "Commit details"}</span>
        <IconButton label="More commit actions" icon={Ellipsis} />
      </div>
      <Show
        when={props.commit.isWip}
        fallback={
          <CommitSummary
            commit={props.commit}
            fileCount={changes.files().length}
            totals={changes.totals()}
          />
        }
      >
        <div class="flex items-center gap-[13px] border-b border-border p-4">
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
              {changes.files().length} files changed on{" "}
              <strong class="text-text-soft">{status.data?.branch ?? "detached HEAD"}</strong>
            </p>
          </div>
        </div>
      </Show>
      <div class="border-b border-border px-2.5 py-3">
        <div class="flex items-center justify-between pt-0 pr-1 pb-2 pl-1">
          <div class="flex items-center gap-1.5">
            <File size={15} />
            <strong class="text-[10px]">Changed files</strong>
            <Badge>{changes.files().length}</Badge>
          </div>
          <Show when={props.commit.isWip}>
            <button
              class="cursor-pointer border-0 bg-transparent text-[9px] text-primary-strong disabled:cursor-default disabled:opacity-50"
              disabled={stage.isPending || changes.files().length === 0}
              onClick={stageAll}
            >
              Stage all
            </button>
          </Show>
        </div>
        <Show when={changes.query.error ?? stage.error}>
          {(error) => (
            <EmptyState
              icon={TriangleAlert}
              title="Something went wrong"
              tone="error"
              class="h-auto py-4"
            >
              {error().message}
            </EmptyState>
          )}
        </Show>
        <ChangedFileList files={changes.files()} />
      </div>
      <Show when={props.commit.isWip}>
        <CommitForm repositoryId={props.commit.repositoryId} />
      </Show>
    </div>
  );
}

function ChangedFileList(props: { files: ChangedFile[] }) {
  return (
    <div class="flex flex-col gap-0.5">
      <For each={props.files}>
        {(file) => (
          <button class="grid w-full cursor-pointer grid-cols-[18px_minmax(0,1fr)_auto] items-center gap-[7px] rounded-md border-0 bg-transparent p-[7px] text-left hover:bg-panel-hover">
            <span
              class={`grid size-[17px] place-items-center rounded-sm font-mono text-[8px] font-bold ${
                file.status === "added" || file.status === "untracked"
                  ? "bg-mint-soft text-mint"
                  : file.status === "deleted" || file.status === "conflicted"
                    ? "bg-coral-soft text-coral"
                    : file.status === "renamed" || file.status === "copied"
                      ? "bg-blue-soft text-blue"
                      : "bg-amber-soft text-amber"
              }`}
            >
              {fileStatusLabel[file.status]}
            </span>
            <span class="flex min-w-0 flex-col gap-0.5">
              <strong class="truncate text-[9.5px] font-[540]">
                {file.path.split("/").slice(-1)[0]}
              </strong>
              <small class="truncate text-[8px] text-faint">
                {file.path.includes("/") ? file.path.slice(0, file.path.lastIndexOf("/")) : "root"}
              </small>
            </span>
            <span class="flex gap-[5px] text-[8px]">
              <Show
                when={file.additions !== null}
                fallback={<em class="text-faint not-italic">binary</em>}
              >
                <em class="text-mint not-italic">+{file.additions}</em>
                <b class="font-medium text-coral">−{file.deletions}</b>
              </Show>
            </span>
          </button>
        )}
      </For>
    </div>
  );
}

function CommitSummary(props: {
  commit: Commit;
  fileCount: number;
  totals: { additions: number; deletions: number };
}) {
  return (
    <div class="border-b border-border p-4">
      <div class="flex items-center gap-[9px]">
        <Avatar initials={props.commit.initials} color={props.commit.avatarColor} size="md" />
        <div class="flex flex-col gap-0.5">
          <strong class="text-[10.5px]">{props.commit.author}</strong>
          <span class="text-[8.5px] text-faint">{props.commit.timestamp}</span>
        </div>
      </div>
      <h2 class="mt-3.5 mb-1.5 text-sm leading-[1.35] tracking-[-.2px]">{props.commit.message}</h2>
      {props.commit.description && (
        <p class="m-0 text-[9.5px] leading-[1.55] text-muted">{props.commit.description}</p>
      )}
      <div class="mt-3 flex w-max items-center overflow-hidden rounded-[5px] border border-border-soft">
        <code class="bg-bg px-[7px] py-1 text-[8.5px] text-text-soft">{props.commit.sha}</code>
        <button
          class="grid h-[23px] w-6 cursor-pointer place-items-center border-0 border-l border-border-soft bg-panel-raised p-0 text-faint"
          aria-label="Copy commit SHA"
        >
          <Copy size={13} />
        </button>
      </div>
      <div class="mt-[13px] flex items-center gap-2 text-[9px] text-muted">
        <span class="mr-auto">
          <strong>{props.fileCount}</strong> files changed
        </span>
        <em class="text-mint not-italic">+{props.totals.additions}</em>
        <b class="font-medium text-coral">−{props.totals.deletions}</b>
      </div>
    </div>
  );
}
