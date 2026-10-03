import type { ChangedFile } from "@gitto/git/types";
import File from "lucide-solid/icons/file";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import { Show } from "solid-js";

import { EmptyState } from "@/components/ui/empty-state";
import { LineStats } from "@/components/ui/line-stats";
import { SectionHeader } from "@/components/ui/section-header";

import { ChangedFileList } from "./changed-file-list";

// Sections shared by the details of a commit and a stash.

/** Why some of the details couldn't be loaded, e.g. "Couldn't load the commit", above the rest. */
export function DetailsError(props: { title: string; error: Error | null | undefined }) {
  return (
    <Show when={props.error} keyed>
      {(error) => (
        <EmptyState
          icon={TriangleAlert}
          title={props.title}
          tone="error"
          class="h-auto border-b border-border py-4"
        >
          {error.message}
        </EmptyState>
      )}
    </Show>
  );
}

/** How many files changed, and the lines added and removed, at the bottom of a summary. */
export function FileTotals(props: {
  count: number;
  totals: { additions: number; deletions: number };
}) {
  return (
    <div class="mt-[13px] flex items-center gap-2 text-[11.5px] text-muted">
      <span class="mr-auto">
        <strong>{props.count}</strong> files changed
      </span>
      <LineStats additions={props.totals.additions} deletions={props.totals.deletions} />
    </div>
  );
}

/** The changed files, under their heading. */
export function ChangedFilesSection(props: {
  files: ChangedFile[];
  /** The details' scroll container, which scrolls the files along with what's above them. */
  scrollElement: HTMLElement | undefined;
}) {
  return (
    <div class="border-b border-border px-2.5 py-3">
      <SectionHeader
        icon={File}
        title="Changed files"
        count={props.files.length}
        class="px-1 pb-2"
      />
      <ChangedFileList files={props.files} scrollElement={props.scrollElement} />
    </div>
  );
}
