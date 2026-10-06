import type { ChangedFile, KeptChange, UncommittedSide } from "@gitto/git/types";
import type { LucideIcon } from "lucide-solid";
import CircleCheck from "lucide-solid/icons/circle-check";
import FilePen from "lucide-solid/icons/file-pen";
import Minus from "lucide-solid/icons/minus";
import Plus from "lucide-solid/icons/plus";
import Trash from "lucide-solid/icons/trash";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import { createEffect, createSignal, on, Show, type JSX } from "solid-js";

import { Badge } from "@/components/ui/badge";
import { IconButton, LinkButton } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { EmptyState } from "@/components/ui/empty-state";
import { Mascot } from "@/components/ui/mascot";
import { Notice } from "@/components/ui/notice";
import { SectionHeader } from "@/components/ui/section-header";
import { UpdatingIndicator } from "@/components/ui/updating-indicator";
import { stagingPaths } from "@/git/changes";
import { shownPathIn, type FileOpener, type UncommittedSource } from "@/git/diff-source";
import { canDiscard, discardAllBlocker, discardDescription, keptMessage } from "@/git/discard";
import { useWorkingTreeChanges } from "@/git/queries/diff";
import { useOperationInProgress } from "@/git/queries/progress";
import { useDiscard, useStage, useUnstage, type DiscardTarget } from "@/git/queries/staging";
import { useHeadSha, useStatus } from "@/git/queries/status";
import { useUnsuspendedData } from "@/git/queries/unsuspended";
import { headLabel } from "@/git/status";
import { useDelayed } from "@/hooks/delayed";
import type { ScrollId } from "@/lib/scroll";

import { ChangedFileList, type FileAction } from "./changed-file-list";
import { CommitForm } from "./commit-form";
import { FileMenu, type FileMenuAction } from "./file-menu";

/**
 * The uncommitted changes: what's staged for the next commit, what isn't, and the commit form. The
 * two file lists split the space evenly and scroll on their own, so moving files between them
 * doesn't shift the layout. A file's changes are discarded from its menu, and all of them from the
 * header, each once that's confirmed: they can't be brought back.
 */
export function WorkingTreeDetails(props: { repositoryId: string; files?: FileOpener }) {
  const changes = useWorkingTreeChanges(() => props.repositoryId);
  const status = useStatus(() => props.repositoryId);
  // Without Suspense, like the changes: the status is refetched whenever a file is saved, and the
  // details would be taken off the page and put back meanwhile, their lists scrolled to the top.
  const summary = useUnsuspendedData(status);
  const stage = useStage(() => props.repositoryId);
  const unstage = useUnstage(() => props.repositoryId);
  const discard = useDiscard(() => props.repositoryId);
  // Without Suspense, like the status: it's refetched whenever anything changes on disk.
  const operationQuery = useOperationInProgress(() => props.repositoryId);
  const operation = useUnsuspendedData(operationQuery);
  // A discard waiting for the edits to a file on show to be saved, before it's run.
  const [preparing, setPreparing] = createSignal(false);
  const busy = () => stage.isPending || unstage.isPending || discard.isPending || preparing();
  // Why staging, unstaging or discarding last failed: each keeps its error until it's run again.
  const lastError = () =>
    [stage, unstage, discard]
      .filter((mutation) => mutation.error)
      .reduce<(typeof stage | typeof discard) | undefined>(
        (latest, mutation) =>
          !latest || mutation.submittedAt > latest.submittedAt ? mutation : latest,
        undefined,
      )?.error;
  // The changes being asked about discarding.
  const [discarding, setDiscarding] = createSignal<DiscardTarget>();
  // The changes discarding them all last kept, as it couldn't discard them.
  const [kept, setKept] = createSignal<KeptChange[]>([]);
  // Another repository's changes aren't this one's.
  createEffect(
    on(
      () => props.repositoryId,
      () => {
        setDiscarding(undefined);
        setKept([]);
        // Nor did discarding them fail.
        discard.reset();
      },
      { defer: true },
    ),
  );
  const discardFile = (side: UncommittedSide): FileMenuAction => ({
    label: "Discard changes…",
    icon: Trash,
    // Not a file's staged changes while an operation is under way, which would go on without them.
    // Not known to be while that loads, or couldn't be read: the git side refuses if one is.
    disabled: (file) => busy() || !canDiscard(file) || (side === "staged" && !!operation()),
    run: (file) => setDiscarding({ file, side }),
  });
  // Why the changes can't all be discarded, if they can't: also while what that depends on loads.
  const discardAllBlocked = () => {
    const current = summary();
    const under = operation();
    if (under === undefined && operationQuery.error) {
      return `Couldn't tell whether an operation is under way: ${operationQuery.error.message}`;
    }
    return current && under !== undefined ? discardAllBlocker(current, under) : "Loading…";
  };
  const lastCommit = useHeadSha(summary);
  // Read once: in JSX, `summary() && headLabel(summary().head)` would check a memo of whether
  // there's data, which a transition (switching repositories) can leave behind the data itself.
  const branch = () => {
    const head = summary()?.head;
    return head && headLabel(head);
  };
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
            On <strong class="text-text-soft">{branch()}</strong>
            {" · "}
            {changes.staged().length} staged, {changes.unstaged().length} unstaged
          </p>
          <Show when={updating()}>
            <UpdatingIndicator class="mt-1" />
          </Show>
        </div>
        <IconButton
          label="Discard all changes…"
          title={discardAllBlocked() ?? "Discard all changes"}
          icon={Trash}
          class="ml-auto self-start"
          disabled={discardAllBlocked() !== undefined || busy()}
          onClick={() => setDiscarding("all")}
        />
      </div>

      <Show when={kept().length}>
        <Notice message={keptMessage(kept())}>
          <LinkButton onClick={() => setKept([])}>Dismiss</LinkButton>
        </Notice>
      </Show>

      <Show when={changes.query.error ?? lastError()} keyed>
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
        repositoryId={props.repositoryId}
        title="Unstaged changes"
        scrollId="unstaged-files"
        source={{ kind: "unstaged" }}
        opener={props.files}
        icon={FilePen}
        files={changes.unstaged()}
        uncounted={changes.uncounted()}
        markerFree={changes.markerFree()}
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
        menu={[discardFile("unstaged")]}
      />
      <FileSection
        repositoryId={props.repositoryId}
        title="Staged changes"
        scrollId="staged-files"
        source={{ kind: "staged" }}
        opener={props.files}
        icon={CircleCheck}
        tone="mint"
        files={changes.staged()}
        uncounted={changes.uncounted()}
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
        menu={[discardFile("staged")]}
      />

      <CommitForm
        repositoryId={props.repositoryId}
        stagedCount={changes.staged().length}
        lastCommit={lastCommit()}
      />

      <ConfirmDialog
        item={discarding()}
        icon={Trash}
        title={(target) => (target === "all" ? "Discard all changes?" : "Discard changes?")}
        description={(target) =>
          target === "all"
            ? "Every staged and unstaged change is lost, and untracked files are deleted. None of it can be brought back."
            : discardDescription(target.file, target.side)
        }
        confirmLabel="Discard"
        // Not while files are being staged, or other changes discarded: it waits for them.
        confirmDisabled={busy()}
        onCancel={() => setDiscarding(undefined)}
        onConfirm={async (target) => {
          setDiscarding(undefined);
          setKept([]);
          const id = props.repositoryId;
          // Once the edits to a file on show are saved: they'd be written back over what's
          // discarded, or kept from it.
          setPreparing(true);
          let saved: boolean;
          try {
            saved = await (props.files?.beforeChange() ?? true);
          } finally {
            setPreparing(false);
          }
          if (!saved || id !== props.repositoryId) return;
          discard.mutate(target, {
            onSuccess: (left) => id === props.repositoryId && setKept(left),
          });
        }}
      />
    </div>
  );
}

function FileSection(props: {
  repositoryId: string;
  title: string;
  icon: LucideIcon;
  tone?: "mint";
  files: ChangedFile[];
  uncounted: boolean;
  /** The conflicted files that are ready to be marked resolved, with no markers left. */
  markerFree?: ReadonlySet<string>;
  /** Marks the list, so it starts at the top again in another repository (see `SCROLL_IDS`). */
  scrollId: ScrollId;
  /** Which side of the uncommitted changes the files are. */
  source: UncommittedSource;
  /** Shows a file's changes when it's clicked. */
  opener?: FileOpener;
  empty: JSX.Element;
  bulkLabel: string;
  busy: boolean;
  onBulk: () => void;
  action: FileAction;
  /** What can be done with a file from its menu, opened by right-clicking it. */
  menu: FileMenuAction[];
}) {
  const [scrollElement, setScrollElement] = createSignal<HTMLDivElement>();
  // The file whose menu is open, by its path.
  const [menuFor, setMenuFor] = createSignal<string>();
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
      <div
        ref={setScrollElement}
        data-scroll-restoration-id={props.scrollId}
        class="min-h-0 flex-1 overflow-y-auto px-2.5 pb-3"
      >
        <Show
          when={props.files.length}
          fallback={<p class="m-0 px-1 pb-1 text-[11.5px] text-faint">{props.empty}</p>}
        >
          <FileMenu
            repositoryId={props.repositoryId}
            files={props.files}
            actions={props.menu}
            onOpenFor={setMenuFor}
          >
            <ChangedFileList
              files={props.files}
              uncounted={props.uncounted}
              markerFree={props.markerFree}
              scrollElement={scrollElement()}
              action={props.action}
              onOpen={props.opener && ((file) => props.opener?.open(props.source, file))}
              openPath={shownPathIn(props.opener, props.source)}
              highlightedPath={menuFor()}
              onPrefetch={
                props.opener &&
                ((file) => props.opener?.prefetch(props.source, file, props.uncounted))
              }
            />
          </FileMenu>
        </Show>
      </div>
    </section>
  );
}
