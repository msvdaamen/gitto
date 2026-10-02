import { cn } from "cn";
import type { JSX } from "solid-js";

import { toneClasses, type Tone } from "./tone";

const borders: Record<Tone, string> = {
  neutral: "border-border",
  purple: "border-[color-mix(in_srgb,var(--primary)_30%,transparent)]",
  blue: "border-[color-mix(in_srgb,var(--blue)_30%,transparent)]",
  mint: "border-border",
  amber: "border-[color-mix(in_srgb,var(--amber)_30%,transparent)]",
  coral: "border-[color-mix(in_srgb,var(--coral)_30%,transparent)]",
};

export function Badge(props: { children: JSX.Element; tone?: Tone }) {
  return (
    <span
      class={cn(
        "inline-flex min-h-[18px] items-center rounded-full border px-1.5 py-px text-[11.5px] leading-none font-[680]",
        toneClasses[props.tone ?? "neutral"],
        borders[props.tone ?? "neutral"],
      )}
    >
      {props.children}
    </span>
  );
}
