import { Switch as KobalteSwitch } from "@kobalte/core/switch";
import type { JSX } from "solid-js";

/** An on/off setting: its label, and what it does underneath, with the switch on the right. */
export function Switch(props: {
  label: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (checked: boolean) => void;
  /** What it does, and anything to know about it now. */
  children?: JSX.Element;
}) {
  return (
    <KobalteSwitch
      class="flex items-start gap-4"
      checked={props.checked}
      disabled={props.disabled}
      onChange={(checked) => props.onChange(checked)}
    >
      <div class="min-w-0 flex-1">
        <KobalteSwitch.Label class="text-[13px] font-[620] text-text">
          {props.label}
        </KobalteSwitch.Label>
        <KobalteSwitch.Description class="mt-1 flex flex-col gap-1.5 text-[12.5px] leading-[1.55] text-text-soft">
          {props.children}
        </KobalteSwitch.Description>
      </div>
      <KobalteSwitch.Input class="peer" />
      <KobalteSwitch.Control class="mt-0.5 flex h-[18px] w-8 shrink-0 cursor-pointer items-center rounded-full border border-border bg-bg p-px transition-colors peer-focus-visible:ring-2 peer-focus-visible:ring-primary data-checked:border-primary data-checked:bg-primary data-disabled:cursor-default data-disabled:opacity-50">
        <KobalteSwitch.Thumb class="size-3.5 rounded-full bg-muted transition-transform data-checked:translate-x-3.5 data-checked:bg-white" />
      </KobalteSwitch.Control>
    </KobalteSwitch>
  );
}
