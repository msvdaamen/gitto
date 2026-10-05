import type { ChangedFile } from "@gitto/git/types";
import Check from "lucide-solid/icons/check";
import Copy from "lucide-solid/icons/copy";
import Ellipsis from "lucide-solid/icons/ellipsis";
import GitCommitHorizontal from "lucide-solid/icons/git-commit-horizontal";
import LoaderCircle from "lucide-solid/icons/loader-circle";
import { createMemo, createSignal, Match, onCleanup, Show, Suspense, Switch } from "solid-js";

import { Avatar } from "@/components/ui/avatar";
import { IconButton } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { lineTotals } from "@/git/changes";
import { useCommitFiles } from "@/git/queries/diff";
import { useCommitDetails } from "@/git/queries/history";
import { stashSha, WIP_ID, type CommitRow } from "@/git/rows";
import { useRelativeTime } from "@/hooks/relative-time";
import type { ScrollId } from "@/lib/scroll";

import { ChangedFilesSection, DetailsError, FileTotals } from "./details-sections";
import { StashDetails } from "./stash-details";
import { WorkingTreeDetails } from "./working-tree-details";

/**
 * Details of the selected history row: a commit, a stash, or the uncommitted changes. Loads what it
 * shows by itself, so it only needs the row's id from the history table.
 */
export function CommitDetails(props: {
  repositoryId: string;
  selectedId: string | undefined;
  /** Shows the changes in one of a commit's files. */
  onOpenFile?: (file: ChangedFile) => void;
  /** The path of the file whose changes are on show, if any. */
  openPath?: string;
}) {
  const [scrollElement, setScrollElement] = createSignal<HTMLDivElement>();
  // The selected row and its repository, kept while the selection is briefly empty: switching
  // repositories clears it until the history picks a row, and effects in the details can still run
  // in between. Kept together, so they never ask one repository for another's commit.
  const selected = createMemo(
    (last: { repositoryId: string; id: string }) =>
      props.selectedId === undefined
        ? last
        : { repositoryId: props.repositoryId, id: props.selectedId },
    { repositoryId: props.repositoryId, id: "" },
  );
  const selectedId = () => selected().id;
  const repositoryId = () => selected().repositoryId;
  const stash = () => stashSha(selectedId());
  // The selected stash; the last one once another row is selected, rather than a <Match>'s narrowed
  // value, which would throw if read by the stash's details before they're removed.
  const shownStash = createMemo((last: string | undefined) => stash() ?? last, undefined);
  return (
    <Show
      when={props.selectedId}
      fallback={<EmptyState icon={GitCommitHorizontal} title="Nothing selected" />}
    >
      <div
        ref={setScrollElement}
        data-scroll-restoration-id={"details" satisfies ScrollId}
        class="flex h-full min-w-[280px] flex-col overflow-y-auto"
      >
        <div class="flex h-[38px] shrink-0 items-center justify-between border-b border-border py-0 pr-[9px] pl-[13px] text-[11.5px] font-[720] tracking-[.07em] text-muted uppercase">
          <span>
            {selectedId() === WIP_ID ? "Working directory" : stash() ? "Stash" : "Commit details"}
          </span>
          <IconButton label="More commit actions" icon={Ellipsis} />
        </div>
        <Suspense fallback={<EmptyState icon={LoaderCircle} loading title="Loading details…" />}>
          <Switch
            fallback={
              <SelectedCommit
                repositoryId={repositoryId()}
                sha={selectedId()}
                scrollElement={scrollElement()}
                onOpenFile={props.onOpenFile}
                openPath={props.openPath}
              />
            }
          >
            <Match when={selectedId() === WIP_ID}>
              <WorkingTreeDetails repositoryId={repositoryId()} />
            </Match>
            <Match when={stash()}>
              <StashDetails
                repositoryId={repositoryId()}
                sha={shownStash()!}
                scrollElement={scrollElement()}
              />
            </Match>
          </Switch>
        </Suspense>
      </div>
    </Show>
  );
}

function SelectedCommit(props: {
  repositoryId: string;
  sha: string;
  /** The details' scroll container, which scrolls the files along with the commit's message. */
  scrollElement: HTMLElement | undefined;
  onOpenFile?: (file: ChangedFile) => void;
  openPath?: string;
}) {
  const details = useCommitDetails(
    () => props.repositoryId,
    () => props.sha,
  );
  const changes = useCommitFiles(
    () => props.repositoryId,
    () => props.sha,
  );
  // The commit and its files are loaded apart, and either can arrive first when another is
  // selected: what's on show is the last commit both are loaded for, so the message is never
  // another commit's than the files. Until one fails, which leaves the other on its own.
  const shown = createMemo((last: { commit: CommitRow; files: ChangedFile[] } | undefined) => {
    if (details.query.error || changes.query.error) return undefined;
    const commit = details.commit();
    return commit && commit.id === changes.shownSha() ? { commit, files: changes.files() } : last;
  }, undefined);
  const files = () => shown()?.files ?? changes.files();
  const totals = createMemo(() => lineTotals(files()));

  // One element, so the file list sees it change size when the message above the files loads.
  return (
    <div>
      <DetailsError
        title="Couldn't load the commit"
        error={details.query.error ?? changes.query.error}
      />
      <Show when={shown()?.commit ?? (changes.query.error && details.commit())} keyed>
        {(commit) => <CommitSummary commit={commit} fileCount={files().length} totals={totals()} />}
      </Show>
      <ChangedFilesSection
        files={files()}
        scrollElement={props.scrollElement}
        // Only once the files on show are this commit's, so another's aren't opened as its.
        onOpen={shown()?.commit.id === props.sha ? props.onOpenFile : undefined}
        openPath={props.openPath}
      />
    </div>
  );
}

function CommitSummary(props: {
  commit: CommitRow;
  fileCount: number;
  totals: { additions: number; deletions: number };
}) {
  const ago = useRelativeTime();

  return (
    <div class="border-b border-border p-4">
      <div class="flex items-center gap-[9px]">
        <Avatar initials={props.commit.initials} color={props.commit.avatarColor} size="md" />
        <div class="flex flex-col gap-0.5">
          <strong class="text-[13px]">{props.commit.author}</strong>
          <span class="text-[11px] text-faint">{ago(props.commit.committedAt)}</span>
        </div>
      </div>
      <h2 class="mt-3.5 mb-1.5 text-sm leading-[1.35] tracking-[-.2px]">{props.commit.message}</h2>
      {props.commit.description && (
        <p class="m-0 text-[12px] leading-[1.55] text-muted">{props.commit.description}</p>
      )}
      <div class="mt-3 flex w-max items-center overflow-hidden rounded-[5px] border border-border-soft">
        <code class="bg-bg px-[7px] py-1 text-[11px] text-text-soft">{props.commit.shortSha}</code>
        <CopyShaButton sha={props.commit.id} />
      </div>
      <FileTotals count={props.fileCount} totals={props.totals} />
    </div>
  );
}

/** How long the copy button says the SHA was copied. */
const COPIED_MS = 1500;

/** Copies the commit's full SHA to the clipboard, and says so for a moment. */
function CopyShaButton(props: { sha: string }) {
  const [copied, setCopied] = createSignal(false);
  let timer: ReturnType<typeof setTimeout> | undefined;
  onCleanup(() => clearTimeout(timer));

  async function copy() {
    await navigator.clipboard.writeText(props.sha);
    setCopied(true);
    clearTimeout(timer);
    timer = setTimeout(() => setCopied(false), COPIED_MS);
  }

  return (
    <button
      class="grid h-[23px] w-6 cursor-pointer place-items-center border-0 border-l border-border-soft bg-panel-raised p-0 text-faint hover:text-text-soft"
      aria-label={copied() ? "Copied the commit SHA" : "Copy commit SHA"}
      title={copied() ? "Copied" : "Copy the full SHA"}
      onClick={() => void copy().catch((error) => console.error("Couldn't copy the SHA", error))}
    >
      {copied() ? <Check size={13} class="text-mint" /> : <Copy size={13} />}
    </button>
  );
}
