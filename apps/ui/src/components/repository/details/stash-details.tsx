import Archive from "lucide-solid/icons/archive";
import { createMemo, Show } from "solid-js";

import { Badge } from "@/components/ui/badge";
import { useStashes, useStashFiles } from "@/git/queries/stash";
import { relativeTime } from "@/lib/format";

import { ChangedFilesSection, DetailsError, FileTotals } from "./details-sections";

/** A stash selected in the history: what git named it, when it was made, and the files it changed. */
export function StashDetails(props: {
  repositoryId: string;
  sha: string;
  /** The details' scroll container, which scrolls the files along with the summary. */
  scrollElement: HTMLElement | undefined;
}) {
  const stashes = useStashes(() => props.repositoryId);
  const changes = useStashFiles(
    () => props.repositoryId,
    () => props.sha,
  );
  // The stash whose files are on show, so the summary never names another one while its files
  // load. Gone once it's popped; the history then selects another row.
  const shown = createMemo(() => stashes.data?.find((stash) => stash.sha === changes.shownSha()));

  // One element, so the file list sees it change size when the summary above the files loads.
  return (
    <div>
      <DetailsError title="Couldn't load the stash" error={stashes.error ?? changes.query.error} />
      <Show when={shown()} keyed>
        {(stash) => (
          <div class="border-b border-border p-4">
            <Badge tone="amber">
              <Archive size={11} class="mr-1" />
              Stash
            </Badge>
            <h2 class="mt-3 mb-1.5 text-sm leading-[1.35] tracking-[-.2px]">{stash.message}</h2>
            <p class="m-0 text-[12px] leading-[1.55] text-muted">
              Stashed {relativeTime(stash.createdAt).toLowerCase()}
            </p>
            <code class="mt-3 inline-block rounded-[5px] border border-border-soft bg-bg px-[7px] py-1 text-[11px] text-text-soft">
              {stash.sha.slice(0, 7)}
            </code>
            <FileTotals count={changes.files().length} totals={changes.totals()} />
          </div>
        )}
      </Show>
      <ChangedFilesSection files={changes.files()} scrollElement={props.scrollElement} />
    </div>
  );
}
