import { cn } from "cn";
import LoaderCircle from "lucide-solid/icons/loader-circle";

/** A spinner saying the data on show is being reloaded; it stays on show meanwhile. */
export function UpdatingIndicator(props: { class?: string }) {
  return (
    <span
      role="status"
      class={cn(
        "inline-flex shrink-0 items-center gap-1 text-[10.5px] font-[650] tracking-normal text-faint normal-case",
        props.class,
      )}
    >
      <LoaderCircle size={10} class="animate-spin motion-reduce:animate-none" />
      Updating…
    </span>
  );
}
