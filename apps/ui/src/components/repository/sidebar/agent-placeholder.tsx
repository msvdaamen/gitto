import { Mascot } from "@/components/ui/mascot";

/** What the sidebar's Agents view shows until agent sessions exist. */
export function AgentPlaceholder() {
  return (
    <div class="flex flex-col items-center px-[18px] py-[65px] text-center max-md:px-0 max-md:py-4 max-md:[&>:not(:first-child)]:hidden">
      <Mascot size={38} />
      <strong class="mt-3 text-[11px]">No agents running</strong>
      <p class="mt-1.5 mb-3 text-[9px] leading-1.5 text-muted">
        Agent sessions will appear here alongside their worktrees.
      </p>
      <button class="h-[27px] cursor-pointer rounded-md border border-border bg-primary-soft px-2.5 text-[9px] text-primary-strong">
        Start a session
      </button>
    </div>
  );
}
