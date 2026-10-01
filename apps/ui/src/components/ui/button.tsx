import { cn } from "cn";
import type { LucideIcon } from "lucide-solid";
import type { JSX } from "solid-js";
import { Dynamic } from "solid-js/web";

export function Button(props: {
  children: JSX.Element;
  icon?: LucideIcon;
  variant?: "primary" | "secondary" | "ghost";
  type?: "button" | "submit";
  disabled?: boolean;
  class?: string;
  onClick?: () => void;
}) {
  const variants = {
    primary:
      "border-[#ad7ada] bg-[linear-gradient(135deg,#c696ed,#9f72db)] text-[#1f1328] shadow-[0_8px_22px_rgba(133,81,186,.2)] enabled:hover:bg-[linear-gradient(135deg,#d0a3f3,#aa7be4)]",
    secondary:
      "bg-panel-raised text-text enabled:hover:border-[color-mix(in_srgb,var(--primary)_45%,var(--border))] enabled:hover:bg-panel-hover",
    ghost: "border-transparent bg-transparent text-text-soft enabled:hover:bg-panel-hover",
  };

  return (
    <button
      type={props.type ?? "button"}
      disabled={props.disabled}
      class={cn(
        "inline-flex h-9 cursor-pointer items-center justify-center gap-2 rounded-lg border border-border px-3.5 text-[12.5px] font-[640] transition-[transform,border-color,background] duration-150 enabled:hover:-translate-y-px focus-ring disabled:cursor-default disabled:opacity-50 motion-reduce:transition-none",
        variants[props.variant ?? "secondary"],
        props.class,
      )}
      onClick={props.onClick}
    >
      {props.icon && <Dynamic component={props.icon} size={16} strokeWidth={1.8} />}
      {props.children}
    </button>
  );
}

/** A borderless, text-only button, for small actions like "View all". */
export function LinkButton(props: {
  children: JSX.Element;
  disabled?: boolean;
  class?: string;
  onClick?: () => void;
}) {
  return (
    <button
      type="button"
      disabled={props.disabled}
      class={cn(
        "cursor-pointer border-0 bg-transparent p-0 text-[9px] text-primary-strong disabled:cursor-default disabled:opacity-50",
        props.class,
      )}
      onClick={props.onClick}
    >
      {props.children}
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
      type="button"
      class={cn(
        "grid size-[30px] shrink-0 cursor-pointer place-items-center rounded-[7px] border border-transparent bg-transparent p-0 text-muted hover:bg-panel-hover hover:text-text focus-ring",
        props.active && "bg-primary-soft text-primary-strong",
        props.class,
      )}
      aria-label={props.label}
      title={props.label}
      onClick={props.onClick}
    >
      <Dynamic component={props.icon} size={16} strokeWidth={1.8} />
    </button>
  );
}
