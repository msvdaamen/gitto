import { cn } from "cn";
import Check from "lucide-solid/icons/check";
import Cloud from "lucide-solid/icons/cloud";
import Laptop from "lucide-solid/icons/laptop";
import Tag from "lucide-solid/icons/tag";
import { createMemo, Show } from "solid-js";

import { toRefLabels, type RefLabel } from "@/git/ref-labels";
import type { CommitRef } from "@/types/git";

/**
 * The commit's branches and tags, tinted in its lane's colour like GitKraken: the first one, with
 * a count of the rest. A branch shows whether it's local (laptop), on a remote (cloud) or both.
 */
export function HistoryRefLabels(props: { refs: CommitRef[]; color: string }) {
  const labels = createMemo(() => toRefLabels(props.refs));

  return (
    <span class="flex items-center gap-1 overflow-hidden" style={{ "--lane": props.color }}>
      <Show when={labels()[0]}>{(label) => <RefPill label={label()} />}</Show>
      <Show when={labels().length > 1}>
        <span
          class="shrink-0 rounded-sm border border-[color-mix(in_srgb,var(--lane)_45%,var(--border))] px-[4px] py-[3px] text-[8px] font-[680] text-text-soft"
          title={labels()
            .slice(1)
            .map((label) => label.name)
            .join("\n")}
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
        "flex min-w-0 items-center gap-[3px] rounded-sm border border-[color-mix(in_srgb,var(--lane)_45%,var(--border))] bg-[color-mix(in_srgb,var(--lane)_16%,transparent)] px-[5px] py-[3px] text-[8px] text-text-soft [&>svg]:shrink-0",
        branch()?.current && "font-[720] text-text",
      )}
      title={props.label.name}
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
