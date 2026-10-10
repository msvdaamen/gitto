import { capitalize } from "@gitto/release/changelog";
import { channelOf } from "@gitto/release/version";
import type { Change, VersionChanges } from "@gitto/system/types";
import { Dialog } from "@kobalte/core/dialog";
import { cn } from "cn";
import Sparkles from "lucide-solid/icons/sparkles";
import { For, onMount, Show } from "solid-js";

import { Badge } from "@/components/ui/badge";
import { Button, LinkButton } from "@/components/ui/button";
import { DIALOG_BOX, DialogPortal } from "@/components/ui/dialog";
import type { Tone } from "@/components/ui/tone";
import { whatsNew } from "@/hooks/whats-new";

const PULL_REQUESTS = "https://github.com/msvdaamen/gitto/pull";

/** How a change is marked: breaking ones first, whatever their type. */
function kindOf(change: Change): { label: string; tone: Tone } {
  if (change.breaking) return { label: "Breaking", tone: "coral" };
  if (change.type === "fix") return { label: "Fixed", tone: "mint" };
  if (change.type === "perf") return { label: "Faster", tone: "blue" };
  return { label: "New", tone: "purple" };
}

/** Breaking changes, then what's new, fixed and faster. */
const ORDER = ["Breaking", "New", "Fixed", "Faster"];

function formatDate(date: string) {
  const day = new Date(`${date}T00:00:00Z`);
  if (Number.isNaN(day.getTime())) return date;
  return day.toLocaleDateString(undefined, {
    day: "numeric",
    month: "short",
    year: "numeric",
    timeZone: "UTC",
  });
}

/**
 * What changed for users in each version since the last one they saw, the newest first: shown
 * once Gitto has updated, and from its version in the footer. Closing it marks them as seen.
 */
export function WhatsNewDialog() {
  let content: HTMLDivElement | undefined;
  onMount(() => void whatsNew.showUnseen());

  return (
    <Dialog open={!!whatsNew.shown()} onOpenChange={(open) => !open && whatsNew.close()} modal>
      <DialogPortal>
        <Dialog.Content
          ref={(el) => (content = el)}
          // The dialog itself, not its first link, a pull request's: there's nothing to answer.
          onOpenAutoFocus={(event) => {
            event.preventDefault();
            content?.focus();
          }}
          class={cn(DIALOG_BOX, "flex max-h-[80vh] max-w-[520px] flex-col p-5 outline-none")}
        >
          <div class="flex items-start gap-3">
            <span class="grid size-9 shrink-0 place-items-center rounded-[9px] bg-primary-soft text-primary-strong">
              <Sparkles size={18} strokeWidth={1.9} />
            </span>
            <div class="min-w-0">
              <Dialog.Title class="m-0 text-[15px] font-[680]">What's new in Gitto</Dialog.Title>
              <Dialog.Description class="m-0 mt-1 text-[12.5px] leading-[1.55] text-text-soft">
                <Show
                  when={whatsNew.shown()?.unseen}
                  fallback="What changed in each version, the newest first."
                >
                  Gitto has updated. Here's what changed since you last looked.
                </Show>
              </Dialog.Description>
            </div>
          </div>

          <div class="-mx-5 mt-4 min-h-0 flex-1 overflow-y-auto border-y border-border px-5 py-1">
            <For
              each={whatsNew.shown()?.versions}
              fallback={
                <p class="my-4 text-[12.5px] text-muted">
                  Nothing to show: this build was made without a changelog.
                </p>
              }
            >
              {(version) => <VersionSection version={version} />}
            </For>
          </div>

          <div class="mt-4 flex justify-end">
            <Button variant="primary" onClick={() => whatsNew.close()}>
              Got it
            </Button>
          </div>
        </Dialog.Content>
      </DialogPortal>
    </Dialog>
  );
}

function VersionSection(props: { version: VersionChanges }) {
  const nightly = () => channelOf(props.version.version) === "nightly";
  const changes = () =>
    props.version.changes.toSorted(
      (a, b) => ORDER.indexOf(kindOf(a).label) - ORDER.indexOf(kindOf(b).label),
    );

  return (
    <section class="border-b border-border py-3 last:border-b-0">
      <h3 class="m-0 flex items-baseline gap-2 text-[13px] font-[680]">
        {nightly() ? "Nightly" : `Gitto ${props.version.version}`}
        <span class="text-[11.5px] font-normal text-faint">{formatDate(props.version.date)}</span>
        <Show when={nightly()}>
          <span class="ml-auto font-mono text-[11px] font-normal text-faint">
            v{props.version.version}
          </span>
        </Show>
      </h3>
      <ul class="m-0 mt-2 flex list-none flex-col gap-2 p-0">
        <For each={changes()}>
          {(change) => (
            <li class="flex items-start gap-2 text-[12.5px] leading-[1.5]">
              <span class="mt-px w-[68px] shrink-0">
                <Badge tone={kindOf(change).tone}>{kindOf(change).label}</Badge>
              </span>
              <div class="min-w-0">
                <span class="text-text">
                  <Show when={change.scope}>
                    {(scope) => <span class="text-muted">{capitalize(scope())}: </span>}
                  </Show>
                  {capitalize(change.description)}
                </span>
                <Show when={change.pr}>
                  {(pr) => (
                    <LinkButton
                      class="ml-1.5 align-baseline"
                      onClick={() => window.open(`${PULL_REQUESTS}/${pr()}`, "_blank")}
                    >
                      #{pr()}
                    </LinkButton>
                  )}
                </Show>
                <Show when={change.detail}>
                  {(detail) => <p class="m-0 mt-0.5 text-text-soft">{detail()}</p>}
                </Show>
              </div>
            </li>
          )}
        </For>
      </ul>
    </section>
  );
}
