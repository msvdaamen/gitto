import { cn } from "cn";
import type { LucideIcon } from "lucide-solid";
import type { JSX } from "solid-js";
import { Dynamic } from "solid-js/web";

export function Button(props: {
  children: JSX.Element;
  icon?: LucideIcon;
  variant?: "primary" | "secondary" | "ghost";
  class?: string;
  onClick?: () => void;
}) {
  const variants = {
    primary:
      "border-[#ad7ada] bg-[linear-gradient(135deg,#c696ed,#9f72db)] text-[#1f1328] shadow-[0_8px_22px_rgba(133,81,186,.2)] hover:bg-[linear-gradient(135deg,#d0a3f3,#aa7be4)]",
    secondary:
      "bg-panel-raised text-text hover:border-[color-mix(in_srgb,var(--primary)_45%,var(--border))] hover:bg-panel-hover",
    ghost: "border-transparent bg-transparent text-text-soft hover:bg-panel-hover",
  };

  return (
    <button
      class={cn(
        "inline-flex h-9 cursor-pointer items-center justify-center gap-2 rounded-lg border border-border px-3.5 text-[12.5px] font-[640] transition-[transform,border-color,background] duration-150 hover:-translate-y-px focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary motion-reduce:transition-none ",
        variants[props.variant ?? "secondary"],
        props.class,
      )}
      onClick={props.onClick}
    >
      {props.icon && <Dynamic component={props.icon} size={16} strokeWidth={1.8} />}
      <span>{props.children}</span>
    </button>
  );
}

export function IconButton(props: {
  label: string;
  icon: LucideIcon;
  active?: boolean;
  class?: string;
  onClick?: (event: MouseEvent) => void;
}) {
  return (
    <button
      class={`grid size-[30px] shrink-0 cursor-pointer place-items-center rounded-[7px] border border-transparent bg-transparent p-0 text-muted hover:bg-panel-hover hover:text-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary ${props.active ? "bg-primary-soft text-primary-strong" : ""} ${props.class ?? ""}`}
      aria-label={props.label}
      title={props.label}
      onClick={props.onClick}
    >
      <Dynamic component={props.icon} size={16} strokeWidth={1.8} />
    </button>
  );
}
