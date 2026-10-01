import Copy from "lucide-solid/icons/copy";
import Ellipsis from "lucide-solid/icons/ellipsis";
import File from "lucide-solid/icons/file";
import GitCommitHorizontal from "lucide-solid/icons/git-commit-horizontal";
import LoaderCircle from "lucide-solid/icons/loader-circle";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import { createSignal, Show, Suspense } from "solid-js";

import { Avatar } from "@/components/ui/avatar";
import { IconButton } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { LineStats } from "@/components/ui/line-stats";
import { SectionHeader } from "@/components/ui/section-header";
import { useCommitFiles } from "@/git/diff";
import { useCommit } from "@/git/history";
import { WIP_ID } from "@/git/rows";
import type { Commit } from "@/types/git";

import { ChangedFileList } from "./changed-file-list";
import { WorkingTreeDetails } from "./working-tree-details";

/**
 * Details of the selected history row: a commit, or the uncommitted changes. Loads what it shows by
 * itself, so it only needs the row's id from the history table.
 */
export function CommitDetails(props: { repositoryId: string; selectedId: string | undefined }) {
  const [scrollElement, setScrollElement] = createSignal<HTMLDivElement>();
  return (
    <Show
      when={props.selectedId}
      fallback={<EmptyState icon={GitCommitHorizontal} title="Nothing selected" />}
    >
      {(selectedId) => (
        <div ref={setScrollElement} class="flex h-full min-w-[280px] flex-col overflow-y-auto">
          <div class="flex h-[38px] shrink-0 items-center justify-between border-b border-border py-0 pr-[9px] pl-[13px] text-[9px] font-[720] tracking-[.07em] text-muted uppercase">
            <span>{selectedId() === WIP_ID ? "Working directory" : "Commit details"}</span>
            <IconButton label="More commit actions" icon={Ellipsis} />
          </div>
          <Suspense fallback={<EmptyState icon={LoaderCircle} title="Loading details…" />}>
            <Show
              when={selectedId() === WIP_ID}
              fallback={
                <SelectedCommit
                  repositoryId={props.repositoryId}
                  sha={selectedId()}
                  scrollElement={scrollElement()}
                />
              }
            >
              <WorkingTreeDetails repositoryId={props.repositoryId} />
            </Show>
          </Suspense>
        </div>
      )}
    </Show>
  );
}

function SelectedCommit(props: {
  repositoryId: string;
  sha: string;
  /** The details' scroll container, which scrolls the files along with the commit's message. */
  scrollElement: HTMLElement | undefined;
}) {
  const details = useCommit(
    () => props.repositoryId,
    () => props.sha,
  );
  const changes = useCommitFiles(
    () => props.repositoryId,
    () => props.sha,
  );

  // One element, so the file list sees it change size when the message above the files loads.
  return (
    <div>
      <Show when={details.query.error ?? changes.query.error}>
        {(error) => (
          <EmptyState
            icon={TriangleAlert}
            title="Couldn't load the commit"
            tone="error"
            class="h-auto border-b border-border py-4"
          >
            {error().message}
          </EmptyState>
        )}
      </Show>
      <Show when={details.commit()}>
        {(commit) => (
          <CommitSummary
            commit={commit()}
            fileCount={changes.files().length}
            totals={changes.totals()}
          />
        )}
      </Show>
      <div class="border-b border-border px-2.5 py-3">
        <SectionHeader
          icon={File}
          title="Changed files"
          count={changes.files().length}
          class="px-1 pb-2"
        />
        <ChangedFileList files={changes.files()} scrollElement={props.scrollElement} />
      </div>
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
        <LineStats additions={props.totals.additions} deletions={props.totals.deletions} />
      </div>
    </div>
  );
}
