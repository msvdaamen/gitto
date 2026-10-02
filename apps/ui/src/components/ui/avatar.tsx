import { cn } from "cn";

export function Avatar(props: { initials: string; color?: string; size?: "sm" | "md" }) {
  return (
    <span
      class={cn(
        "grid shrink-0 place-items-center rounded-full font-[720] text-white shadow-[inset_0_0_0_1px_rgba(255,255,255,.15)]",
        props.size === "md" ? "size-[34px] text-[12.5px]" : "size-[23px] text-[10.5px]",
      )}
      style={{ "background-color": props.color ?? "#8c65cf" }}
    >
      {props.initials}
    </span>
  );
}
