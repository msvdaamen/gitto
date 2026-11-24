import type { JSX } from "@solidjs/web";
import { Field, FieldDescription, FieldLabel } from "@ui-sdk/solid/field";
import { Switch as SwitchControl, SwitchThumb } from "@ui-sdk/solid/switch";

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
    <Field class="flex items-start gap-4" disabled={props.disabled}>
      <div class="min-w-0 flex-1">
        <FieldLabel class="text-[13px] font-[620] text-text">{props.label}</FieldLabel>
        <FieldDescription class="mt-1 flex flex-col gap-1.5 text-[12.5px] leading-[1.55] text-text-soft">
          {props.children}
        </FieldDescription>
      </div>
      <SwitchControl
        class="mt-0.5 flex h-[18px] w-8 shrink-0 cursor-pointer items-center rounded-full border border-border bg-bg p-px transition-colors outline-none focus-visible:ring-2 focus-visible:ring-primary data-checked:border-primary data-checked:bg-primary data-disabled:cursor-default data-disabled:opacity-50"
        checked={props.checked}
        disabled={props.disabled}
        onCheckedChange={(checked) => props.onChange(checked)}
      >
        <SwitchThumb class="size-3.5 rounded-full bg-muted transition-transform data-checked:translate-x-3.5 data-checked:bg-white" />
      </SwitchControl>
    </Field>
  );
}
