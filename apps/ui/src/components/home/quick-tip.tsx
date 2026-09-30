import Command from "lucide-solid/icons/command";

import { Kbd } from "@/components/ui/kbd";

import { demoAction } from "./demo-action";

export function QuickTip() {
  return (
    <button
      class="col-span-full flex w-full cursor-pointer items-start gap-2.5 rounded-[10px] border border-dashed border-[color-mix(in_srgb,var(--primary)_38%,var(--border))] bg-primary-soft p-3 text-left text-text"
      onClick={demoAction("Command palette is coming next.")}
    >
      <span class="grid size-7 place-items-center rounded-[7px] bg-panel text-primary-strong">
        <Command size={17} />
      </span>
      <div class="flex-1">
        <strong class="text-[10.5px]">Quick tip</strong>
        <p class="mt-1 mb-0 text-[9.5px] leading-1.5 text-muted">
          Press <Kbd>⌘ K</Kbd> to open the command palette.
        </p>
      </div>
    </button>
  );
}
