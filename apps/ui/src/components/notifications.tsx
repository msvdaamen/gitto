import { Popover } from "@kobalte/core/popover";
import Bell from "lucide-solid/icons/bell";
import BellOff from "lucide-solid/icons/bell-off";
import Download from "lucide-solid/icons/download";
import RotateCw from "lucide-solid/icons/rotate-cw";
import { createSignal, Show } from "solid-js";

import { useUpdate } from "@/hooks/update";
import { rpc } from "@/lib/rpc";

import { Button } from "./ui/button";

/**
 * The bell, which opens what Gitto has to tell the user: for now, a newer version it's getting.
 * Its dot says there's something the user hasn't opened the bell to see since it last changed.
 */
export function Notifications() {
  const state = useUpdate();
  const update = () => state()?.update ?? null;
  // What the user last saw: an update, while downloading, is news again once it's ready.
  const key = () => {
    const current = update();
    return current && `${current.version}:${current.ready}`;
  };
  const [seen, setSeen] = createSignal<string | null>(null);
  const unseen = () => !!key() && key() !== seen();

  return (
    <Popover placement="bottom-end" gutter={6} onOpenChange={(open) => open && setSeen(key())}>
      <span class="relative flex max-sm:hidden">
        <Popover.Trigger
          class="grid size-[30px] shrink-0 cursor-pointer place-items-center rounded-[7px] border border-transparent bg-transparent p-0 text-muted hover:bg-panel-hover hover:text-text focus-ring data-expanded:bg-panel-hover data-expanded:text-text"
          aria-label={unseen() ? "Notifications (new)" : "Notifications"}
          title="Notifications"
        >
          <Bell size={16} strokeWidth={1.8} />
        </Popover.Trigger>
        <Show when={unseen()}>
          <i class="pointer-events-none absolute top-1.25 right-1.25 size-1.25 rounded-full bg-coral shadow-[0_0_0_2px_var(--bg-soft)]" />
        </Show>
      </span>
      <Popover.Portal>
        <Popover.Content class="z-50 w-[300px] animate-toast-in rounded-lg border border-border bg-panel-raised p-1 text-text shadow-app outline-none motion-reduce:animate-none">
          <Popover.Title class="m-0 px-2 pt-1.5 pb-1 text-[12.5px] font-[680]">
            Notifications
          </Popover.Title>
          <Show
            when={update()}
            keyed
            fallback={
              <div class="flex flex-col items-center gap-1.5 px-4 py-6 text-center text-faint">
                <BellOff size={18} />
                <span class="text-[12px]">You're all caught up</span>
              </div>
            }
          >
            {(current) => (
              <div class="flex items-start gap-2.5 rounded-[5px] p-2">
                <span class="grid size-7 shrink-0 place-items-center rounded-full bg-primary-soft text-primary-strong">
                  <Show when={current.ready} fallback={<Download size={14} />}>
                    <RotateCw size={14} />
                  </Show>
                </span>
                <div class="flex min-w-0 flex-col gap-1">
                  <strong class="text-[12.5px] font-[640]">
                    Gitto {current.version} is available
                  </strong>
                  <Show
                    when={current.ready}
                    fallback={<span class="text-[11.5px] text-muted">Downloading the update…</span>}
                  >
                    <span class="text-[11.5px] text-muted">
                      It's downloaded, and installed when you restart Gitto.
                    </span>
                    <Button
                      variant="primary"
                      class="mt-1 h-7 self-start px-2.5 text-[12px]"
                      onClick={() => void rpc.system.update.install()}
                    >
                      Restart to update
                    </Button>
                  </Show>
                </div>
              </div>
            )}
          </Show>
        </Popover.Content>
      </Popover.Portal>
    </Popover>
  );
}
