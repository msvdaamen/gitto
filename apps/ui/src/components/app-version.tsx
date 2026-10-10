import RotateCw from "lucide-solid/icons/rotate-cw";
import { Show } from "solid-js";

import { useUpdate } from "@/hooks/update";
import { whatsNew } from "@/hooks/whats-new";
import { rpc } from "@/lib/rpc";

import { LinkButton } from "./ui/button";
import { Divider } from "./ui/divider";

/**
 * Which Gitto this is, which opens what's new in it, and the update it's getting: restarting into
 * it, once it's downloaded.
 */
export function AppVersion() {
  const state = useUpdate();

  return (
    <Show when={state()} keyed>
      {(current) => (
        <>
          <Show when={current.update} keyed>
            {(update) => (
              <>
                <Show
                  when={update.ready}
                  fallback={
                    <span class="text-faint" title={`Downloading Gitto ${update.version}`}>
                      Downloading update…
                    </span>
                  }
                >
                  <LinkButton
                    class="flex items-center gap-1.25 text-[13px]"
                    onClick={() => void rpc.system.update.install()}
                  >
                    <RotateCw size={12} />
                    Restart to update to {update.version}
                  </LinkButton>
                </Show>
                <Divider />
              </>
            )}
          </Show>
          <button
            type="button"
            title="What's new"
            class="flex cursor-pointer items-center gap-1.25 rounded border-0 bg-transparent p-0 text-inherit hover:text-text focus-ring"
            onClick={() => void whatsNew.showAll()}
          >
            <span>{current.channel === "nightly" ? "Gitto Nightly" : "Gitto"}</span>
            <span class="font-mono text-faint">v{current.version}</span>
          </button>
        </>
      )}
    </Show>
  );
}
