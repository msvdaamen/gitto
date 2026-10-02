import { cn } from "cn";
import LoaderCircle from "lucide-solid/icons/loader-circle";

/**
 * A spinner saying the data on show is being reloaded (or, with a `label`, what else is going on);
 * it stays on show meanwhile.
 */
export function UpdatingIndicator(props: { class?: string; label?: string }) {
  return (
    <span
      role="status"
      class={cn(
        "inline-flex shrink-0 items-center gap-1 text-[8px] font-[650] tracking-normal text-faint normal-case",
        props.class,
      )}
    >
      <LoaderCircle size={10} class="animate-spin motion-reduce:animate-none" />
      {props.label ?? "Updating…"}
    </span>
  );
}
