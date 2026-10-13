import { createResource, createSignal, Show } from "solid-js";

import { Switch } from "@/components/ui/switch";
import { rpc } from "@/lib/rpc";

/** Whether Gitto sends error reports. */
export function TelemetrySetting() {
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
    // Not keyed: the switch stays as it is, focus and all, when the state changes.
    <Show when={state()}>
      {(current) => (
        <Switch
          label="Send error reports"
          checked={current().enabled}
          disabled={!current().available || saving()}
          onChange={(enabled) => void setEnabled(enabled)}
        >
          <span>
            When something goes wrong, Gitto tells its developers what and where. Reports can name
            files and branches, and include Git's errors, but not what's in your files. Paths start
            from your home folder, ~, and passwords in URLs are left out.
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
        </Switch>
      )}
    </Show>
  );
}
