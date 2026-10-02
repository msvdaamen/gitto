import { cn } from "cn";
import Check from "lucide-solid/icons/check";
import Cloud from "lucide-solid/icons/cloud";
import Laptop from "lucide-solid/icons/laptop";
import Tag from "lucide-solid/icons/tag";
import { createMemo, Show } from "solid-js";

import { describeRefLabel, refLabelMatches, type RefLabel } from "@/git/ref-labels";

/** A ref pill's shape, border and text, in its lane's colour (`--lane`). */
const PILL =
  "rounded-sm border border-[color-mix(in_srgb,var(--lane)_45%,var(--border))] py-[3px] text-[8px] text-text-soft";

/**
 * The commit's branches and tags, tinted in its lane's colour like GitKraken: the first one, with
 * a count of the rest. A branch shows whether it's local (laptop), on a remote (cloud) or both.
 * While searching (`search`, lowercase), a label the search matches comes first, so the row shows
 * why it matched.
 */
export function HistoryRefLabels(props: { labels: RefLabel[]; search: string; color: string }) {
  const labels = createMemo(() => {
    const all = props.labels;
    const match = props.search
      ? all.findIndex((label) => refLabelMatches(label, props.search))
      : -1;
    return match > 0 ? [all[match]!, ...all.toSpliced(match, 1)] : all;
  });

  return (
    <span class="flex items-center gap-1 overflow-hidden" style={{ "--lane": props.color }}>
      <Show when={labels()[0]} keyed>
        {(label) => <RefPill label={label} />}
      </Show>
      <Show when={labels().length > 1}>
        <span
          class={cn(PILL, "shrink-0 px-[4px] font-[680]")}
          title={labels().slice(1).map(describeRefLabel).join("\n")}
        >
          +{labels().length - 1}
        </span>
      </Show>
    </span>
  );
}

function RefPill(props: { label: RefLabel }) {
  const branch = () => (props.label.kind === "branch" ? props.label : undefined);

  return (
    <span
      class={cn(
        PILL,
        "flex min-w-0 items-center gap-[3px] bg-[color-mix(in_srgb,var(--lane)_16%,transparent)] px-[5px] [&>svg]:shrink-0",
        branch()?.current && "font-[720] text-text",
      )}
      title={describeRefLabel(props.label)}
    >
      <Show when={branch()?.current}>
        <Check size={9} strokeWidth={3} />
      </Show>
      <Show when={props.label.kind === "tag"}>
        <Tag size={9} strokeWidth={2.4} />
      </Show>
      <span class="truncate">{props.label.name}</span>
      <Show when={branch()?.local}>
        <Laptop size={9} strokeWidth={2.4} aria-label="Local" />
      </Show>
      <Show when={branch()?.remotes.length}>
        <Cloud size={9} strokeWidth={2.4} aria-label={`On ${branch()!.remotes.join(", ")}`} />
      </Show>
    </span>
  );
}
