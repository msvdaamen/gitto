import Cloud from "lucide-solid/icons/cloud";
import GitBranch from "lucide-solid/icons/git-branch";
import { Show } from "solid-js";

import { Divider } from "./ui/divider";
import { GittoIcon } from "./ui/gitto-icon";
import { StatusDot } from "./ui/status-dot";

export function Footer() {
  return (
    <footer class="flex items-center justify-between gap-5 border-t border-border bg-[color-mix(in_srgb,var(--bg-soft)_94%,var(--primary)_6%)] px-2.75 text-[10.5px] text-muted">
      <div class="flex items-center gap-1.25 whitespace-nowrap [&>span]:flex [&>span]:items-center [&>span]:gap-1.25">
        <GittoIcon class="h-3 w-3.5" />
        <StatusDot color="var(--mint)" />
        <span>Ready</span>
        <Show when={true}>
          <Divider />
          <span>
            <GitBranch size={12} />
            BRANCHNAME
          </span>
          <span class="text-blue">
            <Cloud size={12} />
            origin · synced
          </span>
        </Show>
      </div>
      <div class="flex items-center gap-1.25 whitespace-nowrap [&>span]:flex [&>span]:items-center [&>span]:gap-1.25">
        <Show when={true}>
          <span class="text-mint">
            <StatusDot color="var(--mint)" /> Repository healthy
          </span>
          <Divider />
        </Show>
        <span>Gitto Preview</span>
        <span class="font-mono text-faint">v0.1.0</span>
      </div>
    </footer>
  );
}
