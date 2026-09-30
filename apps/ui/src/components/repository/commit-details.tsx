import Copy from "lucide-solid/icons/copy";
import Ellipsis from "lucide-solid/icons/ellipsis";
import File from "lucide-solid/icons/file";
import GitCommitHorizontal from "lucide-solid/icons/git-commit-horizontal";
import LoaderCircle from "lucide-solid/icons/loader-circle";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import { Show, Suspense } from "solid-js";

import { Avatar } from "@/components/ui/avatar";
import { Badge } from "@/components/ui/badge";
import { IconButton } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { useCommitFiles } from "@/git/diff";
import { useHistory } from "@/git/history";
import type { Commit } from "@/types/git";

import { ChangedFileList } from "./changed-file-list";
import { WorkingTreeDetails } from "./working-tree-details";

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
  return (
    <div class="flex h-full min-w-[280px] flex-col overflow-y-auto">
      <div class="flex h-[38px] shrink-0 items-center justify-between border-b border-border py-0 pr-[9px] pl-[13px] text-[9px] font-[720] tracking-[.07em] text-muted uppercase">
        <span>{props.commit.isWip ? "Working directory" : "Commit details"}</span>
        <IconButton label="More commit actions" icon={Ellipsis} />
      </div>
      <Show when={props.commit.isWip} fallback={<CommitFiles commit={props.commit} />}>
        <WorkingTreeDetails repositoryId={props.commit.repositoryId} />
      </Show>
    </div>
  );
}

function CommitFiles(props: { commit: Commit }) {
  const changes = useCommitFiles(() => props.commit);

  return (
    <>
      <CommitSummary
        commit={props.commit}
        fileCount={changes.files().length}
        totals={changes.totals()}
      />
      <div class="border-b border-border px-2.5 py-3">
        <div class="flex items-center gap-1.5 pt-0 pr-1 pb-2 pl-1">
          <File size={15} />
          <strong class="text-[10px]">Changed files</strong>
          <Badge>{changes.files().length}</Badge>
        </div>
        <Show when={changes.query.error}>
          {(error) => (
            <EmptyState
              icon={TriangleAlert}
              title="Couldn't load changed files"
              tone="error"
              class="h-auto py-4"
            >
              {error().message}
            </EmptyState>
          )}
        </Show>
        <ChangedFileList files={changes.files()} />
      </div>
    </>
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
