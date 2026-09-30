import { cn } from "cn";

/**
 * Gitto's blob, as a friendly face on empty panels. Everything scales with `size` (its width in
 * pixels).
 */
export function Mascot(props: { size: number; class?: string }) {
  return (
    <span
      class={cn(
        "inline-flex h-[.85em] w-[1em] shrink-0 items-center justify-center gap-[.2em] rounded-[45%_55%_53%_47%/59%_43%_57%_41%] bg-[linear-gradient(145deg,#bc8aef,#7861e6)]",
        props.class,
      )}
      style={{ "font-size": `${props.size}px` }}
      aria-hidden="true"
    >
      <span class="size-[.08em] rounded-full bg-[#2b2032]" />
      <span class="size-[.08em] rounded-full bg-[#2b2032]" />
    </span>
  );
}
