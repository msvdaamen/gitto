import { TextField } from "@kobalte/core/text-field";
import { cn } from "cn";
import type { LucideIcon } from "lucide-solid";
import { Dynamic } from "solid-js/web";

export function TextInput(props: {
  value?: string;
  placeholder?: string;
  label?: string;
  onChange?: (value: string) => void;
  compact?: boolean;
  icon?: LucideIcon;
}) {
  return (
    <TextField
      class={cn(
        "flex items-center gap-2 rounded-lg border border-border px-2 pr-2 pl-2.5 text-muted focus-within:border-[color-mix(in_srgb,var(--primary)_62%,var(--border))] focus-within:shadow-[0_0_0_3px_var(--primary-soft)]",
        props.compact
          ? "h-7.5 w-[min(210px,17vw)] bg-bg max-sm:w-full"
          : "h-8.5 w-56 bg-panel",
      )}
      value={props.value}
      onChange={props.onChange}
    >
      {props.label && <TextField.Label>{props.label}</TextField.Label>}
      {props.icon ? <Dynamic component={props.icon} size={15} strokeWidth={1.8} /> : undefined}
      <TextField.Input
        class="w-full min-w-0 border-0 bg-transparent text-text outline-0 placeholder:text-faint"
        placeholder={props.placeholder}
      />
    </TextField>
  );
}
