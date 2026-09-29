import type { JSX } from "solid-js";

export function Badge(props: {
  children: JSX.Element;
  tone?: "purple" | "blue" | "mint" | "amber" | "neutral";
}) {
  const tones = {
    neutral: "border-border bg-panel-raised text-muted",
    purple:
      "border-[color-mix(in_srgb,var(--primary)_30%,transparent)] bg-primary-soft text-primary-strong",
    blue: "border-[color-mix(in_srgb,var(--blue)_30%,transparent)] bg-blue-soft text-blue",
    mint: "border-border bg-mint-soft text-mint",
    amber: "border-[color-mix(in_srgb,var(--amber)_30%,transparent)] bg-amber-soft text-amber",
  };
  return (
    <span
      class={`inline-flex min-h-[18px] items-center rounded-full border px-1.5 py-px text-[9px] leading-none font-[680] ${tones[props.tone ?? "neutral"]}`}
    >
      {props.children}
    </span>
  );
}
