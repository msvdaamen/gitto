import { Popover } from "@kobalte/core/popover";
import { cn } from "cn";
import Check from "lucide-solid/icons/check";
import Cloud from "lucide-solid/icons/cloud";
import Laptop from "lucide-solid/icons/laptop";
import Tag from "lucide-solid/icons/tag";
import { createMemo, createSignal, For, onCleanup, Show } from "solid-js";

import { describeRefLabel, refLabelMatches, type RefLabel } from "@/git/ref-labels";

/** A ref pill's shape, border and text, in its lane's colour (`--lane`). */
const PILL =
  "rounded-sm border border-[color-mix(in_srgb,var(--lane)_45%,var(--border))] py-[3px] text-[10.5px] text-text-soft";

/**
 * The commit's branches and tags, tinted in its lane's colour like GitKraken: the first one, with
 * a count of the rest, which hovering it lists. A branch shows whether it's local (laptop), on a
 * remote (cloud) or both. While searching (`search`, lowercase), a label the search matches comes
 * first, so the row shows why it matched. Double-clicking a branch switches to it, like in the
 * sidebar (see `onSwitch`).
 */
export function HistoryRefLabels(props: {
  labels: RefLabel[];
  search: string;
  color: string;
  /** Switches to a branch, by its full ref name. */
  onSwitch: (ref: string) => void;
}) {
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
        {(label) => <RefPill label={label} onSwitch={props.onSwitch} />}
      </Show>
      <Show when={labels().length > 1}>
        <MoreLabels labels={labels().slice(1)} color={props.color} onSwitch={props.onSwitch} />
      </Show>
    </span>
  );
}

/** How long the count is hovered before the rest show, so moving across the history doesn't flash them. */
const OPEN_DELAY_MS = 150;
/** How long the rest stay once the pointer leaves, so it can move from the count onto them. */
const CLOSE_DELAY_MS = 200;

/** The count of the labels a row has no room for, listing them under it while it's hovered. */
function MoreLabels(props: { labels: RefLabel[]; color: string; onSwitch: (ref: string) => void }) {
  const [open, setOpen] = createSignal(false);
  let timer: ReturnType<typeof setTimeout> | undefined;
  const openSoon = () => {
    clearTimeout(timer);
    timer = setTimeout(() => setOpen(true), OPEN_DELAY_MS);
  };
  const closeSoon = () => {
    clearTimeout(timer);
    timer = setTimeout(() => setOpen(false), CLOSE_DELAY_MS);
  };
  onCleanup(() => clearTimeout(timer));

  return (
    <Popover open={open()} onOpenChange={setOpen} placement="bottom-start" gutter={4}>
      <Popover.Anchor
        as="span"
        class={cn(PILL, "shrink-0 px-[4px] font-[680]")}
        onPointerEnter={openSoon}
        onPointerLeave={closeSoon}
      >
        +{props.labels.length}
      </Popover.Anchor>
      <Popover.Portal>
        <Popover.Content
          // Neither takes the focus from the history, nor hands it back: it's only hovered.
          onOpenAutoFocus={(event) => event.preventDefault()}
          onCloseAutoFocus={(event) => event.preventDefault()}
          onPointerEnter={() => clearTimeout(timer)}
          onPointerLeave={closeSoon}
          class="z-50 flex max-w-[320px] flex-col items-start gap-1 rounded-md border border-border bg-panel-raised p-1.5 shadow-app outline-none"
          style={{ "--lane": props.color }}
        >
          <For each={props.labels}>
            {(label) => <RefPill label={label} onSwitch={props.onSwitch} />}
          </For>
        </Popover.Content>
      </Popover.Portal>
    </Popover>
  );
}

function RefPill(props: { label: RefLabel; onSwitch: (ref: string) => void }) {
  const branch = () => (props.label.kind === "branch" ? props.label : undefined);
  // Only a branch that isn't checked out says it can be switched to; `onSwitch` skips the other.
  const title = () =>
    branch() && !branch()!.current
      ? `${describeRefLabel(props.label)}\nDouble-click to switch to it`
      : describeRefLabel(props.label);

  function onDblClick() {
    const label = branch();
    if (label) props.onSwitch(label.ref);
  }

  return (
    <span
      class={cn(
        PILL,
        "flex min-w-0 items-center gap-[3px] bg-[color-mix(in_srgb,var(--lane)_16%,transparent)] px-[5px] [&>svg]:shrink-0",
        branch()?.current && "font-[720] text-text",
      )}
      title={title()}
      onDblClick={onDblClick}
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
