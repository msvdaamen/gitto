import { Dialog } from "@kobalte/core/dialog";
import { Switch } from "@kobalte/core/switch";
import { cn } from "cn";
import Settings from "lucide-solid/icons/settings";
import { createResource, createSignal, Show } from "solid-js";

import { Button } from "@/components/ui/button";
import { DIALOG_BOX, DialogPortal } from "@/components/ui/dialog";
import { rpc } from "@/lib/rpc";

/** Gitto's settings: for now, whether it sends error reports. */
export function SettingsDialog(props: { open: boolean; onOpenChange: (open: boolean) => void }) {
  return (
    <Dialog open={props.open} onOpenChange={props.onOpenChange} modal>
      <DialogPortal>
        <Dialog.Content class={cn(DIALOG_BOX, "max-w-[460px] p-5 outline-none")}>
          <div class="flex items-start gap-3">
            <span class="grid size-9 shrink-0 place-items-center rounded-[9px] bg-primary-soft text-primary-strong">
              <Settings size={18} strokeWidth={1.9} />
            </span>
            <div class="min-w-0">
              <Dialog.Title class="m-0 text-[15px] font-[680]">Settings</Dialog.Title>
              <Dialog.Description class="m-0 mt-1 text-[12.5px] leading-[1.55] text-text-soft">
                They apply to Gitto on this computer.
              </Dialog.Description>
            </div>
          </div>

          <section class="mt-4 border-t border-border pt-4">
            <h3 class="m-0 text-[11.5px] font-[680] tracking-wide text-muted uppercase">Privacy</h3>
            <TelemetrySwitch />
          </section>

          <div class="mt-5 flex justify-end">
            <Button variant="primary" onClick={() => props.onOpenChange(false)}>
              Done
            </Button>
          </div>
        </Dialog.Content>
      </DialogPortal>
    </Dialog>
  );
}

/** Whether Gitto sends error reports and how long it takes to start. */
function TelemetrySwitch() {
  const [state, { mutate }] = createResource(() => rpc.system.telemetry.get());
  const [saving, setSaving] = createSignal(false);
  const [failed, setFailed] = createSignal(false);

  const setEnabled = async (enabled: boolean) => {
    setSaving(true);
    setFailed(false);
    try {
      mutate(await rpc.system.telemetry.set({ enabled }));
    } catch (error) {
      console.error("Couldn't change whether Gitto sends error reports", error);
      setFailed(true);
    } finally {
      setSaving(false);
    }
  };

  return (
    <Show when={state()}>
      {(current) => (
        <Switch
          class="mt-3 flex items-start gap-3"
          checked={current().enabled}
          disabled={!current().available || saving()}
          onChange={(enabled) => void setEnabled(enabled)}
        >
          <div class="min-w-0 flex-1">
            <Switch.Label class="text-[13px] font-[620] text-text">Send error reports</Switch.Label>
            <Switch.Description class="mt-1 flex flex-col gap-1.5 text-[12.5px] leading-[1.55] text-text-soft">
              <span>
                When something goes wrong, Gitto tells its developers what and where, and how long
                it took to start. Reports can name files and branches, and include Git's errors, but
                not what's in your files. Paths start from your home folder, ~, and passwords in
                URLs are left out.
              </span>
              <Show when={!current().available}>
                <span class="text-faint">This build of Gitto doesn't send error reports.</span>
              </Show>
              <Show when={current().available && current().enabled && !current().sending}>
                <span class="text-faint">Gitto starts sending them when it restarts.</span>
              </Show>
              <Show when={failed()}>
                <span role="status" class="text-coral">
                  Couldn't save the setting. Try again.
                </span>
              </Show>
            </Switch.Description>
          </div>
          <Switch.Input class="peer" />
          <Switch.Control class="mt-0.5 flex h-[18px] w-8 shrink-0 cursor-pointer items-center rounded-full border border-border bg-bg p-px transition-colors peer-focus-visible:ring-2 peer-focus-visible:ring-primary data-checked:border-primary data-checked:bg-primary data-disabled:cursor-default data-disabled:opacity-50">
            <Switch.Thumb class="size-3.5 rounded-full bg-muted transition-transform data-checked:translate-x-3.5 data-checked:bg-white" />
          </Switch.Control>
        </Switch>
      )}
    </Show>
  );
}
