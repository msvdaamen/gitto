import { cn } from "cn";

/** Lines added and removed, like `+12 −3`. */
export function LineStats(props: { additions: number; deletions: number; class?: string }) {
  return (
    <span class={cn("flex items-center gap-[5px]", props.class)}>
      <span class="text-mint">+{props.additions}</span>
      <span class="font-medium text-coral">−{props.deletions}</span>
    </span>
  );
}
