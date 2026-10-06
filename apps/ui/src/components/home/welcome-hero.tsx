import ArrowDownUp from "lucide-solid/icons/arrow-down-up";
import Check from "lucide-solid/icons/check";
import Copy from "lucide-solid/icons/copy";
import Folder from "lucide-solid/icons/folder";
import Plus from "lucide-solid/icons/plus";
import Sparkles from "lucide-solid/icons/sparkles";
import { createMemo, Show } from "solid-js";

import { Button } from "@/components/ui/button";
import { FailurePopover } from "@/components/ui/failure-popover";
import { useRelativeTime } from "@/hooks/relative-time";

import { demoAction } from "./demo-action";
import type { RepositorySummary } from "./summaries";

/** The home page banner: a greeting, a way to open a repository, and whether all are in sync. */
export function WelcomeHero(props: {
  /** The name the user commits with; `null` if they haven't set one, `undefined` until loaded. */
  userName: string | null | undefined;
  summaries: RepositorySummary[];
  onOpenRepository: () => void;
  /** Why the last repository couldn't be opened, under the button, until it's dismissed. */
  openError: Error | null;
  onDismissOpenError: () => void;
}) {
  // The first of their names, the way people are greeted.
  const firstName = () => props.userName?.trim().split(/\s+/)[0];

  return (
    <section class="relative mx-auto mb-5.5 flex min-h-52.5 max-w-322.5 items-center overflow-hidden rounded-[13px] border border-[color-mix(in_srgb,var(--primary)_30%,var(--border))] bg-[linear-gradient(122deg,color-mix(in_srgb,var(--panel)_90%,var(--primary)_10%),var(--panel))] px-9.5 py-8 shadow-app after:pointer-events-none after:absolute after:inset-0 after:bg-[radial-gradient(color-mix(in_srgb,var(--primary)_55%,transparent)_.65px,transparent_.65px)] after:bg-size-[17px_17px] after:opacity-[.18] after:mask-[linear-gradient(90deg,transparent,#000)] after:content-[''] max-sm:min-h-57.5 max-sm:p-6.5">
      <HeroDecoration />
      <div class="relative z-2">
        <div class="mb-3 flex items-center gap-1.75 text-[12.5px] font-[720] tracking-[.11em] text-primary-strong uppercase">
          <span class="grid size-5.75 place-items-center rounded-[7px] bg-primary-soft text-primary-strong">
            <Sparkles size={13} />
          </span>{" "}
          Your workspace
        </div>
        <h1 class="m-0 text-[clamp(27px,2.6vw,38px)] leading-[1.1] tracking-[-1.25px]">
          {firstName() ? `Welcome back, ${firstName()}.` : "Welcome back."}
        </h1>
        <p class="mt-2.25 mb-5 text-sm text-muted">
          {props.summaries.length
            ? "Pick up where you left off, or shape something new."
            : "Open a git repository to get started, or shape something new."}
        </p>
        <div class="flex items-center gap-2 max-sm:flex-wrap">
          <FailurePopover
            title="Open repository"
            error={props.openError}
            onDismiss={props.onDismissOpenError}
          >
            <Button variant="primary" icon={Folder} onClick={props.onOpenRepository}>
              Open repository
            </Button>
          </FailurePopover>
          <Button
            icon={Copy}
            onClick={demoAction("Clone repository is ready for Git integration.")}
          >
            Clone from URL
          </Button>
          <Button
            variant="ghost"
            icon={Plus}
            onClick={demoAction("Initialize repository is a demo action.")}
          >
            New repository
          </Button>
        </div>
      </div>
      <SyncStatus summaries={props.summaries} />
    </section>
  );
}

/** The big mascot peeking in from the top, and a blue blob in the corner. */
function HeroDecoration() {
  return (
    <>
      <div class="pointer-events-none absolute -top-10.75 right-27.5 h-38 w-45 rotate-12 rounded-[44%_56%_62%_38%/47%_38%_62%_53%] bg-[linear-gradient(145deg,var(--primary),#5c62d5)] opacity-30 blur-[.2px] before:absolute before:top-18 before:left-15.75 before:size-1.75 before:rounded-full before:bg-[rgba(30,21,37,.8)] before:shadow-[31px_0_rgba(30,21,37,.8)] before:content-[''] max-md:right-2.5 [&>span:first-child]:absolute [&>span:first-child]:top-23.5 [&>span:first-child]:left-18.5 [&>span:first-child]:h-1.75 [&>span:first-child]:w-5.25 [&>span:first-child]:rounded-b-[50%] [&>span:first-child]:border-b-2 [&>span:first-child]:border-[rgba(30,21,37,.7)]">
        <span />
        <span />
      </div>
      <div class="pointer-events-none absolute -right-7 -bottom-9.5 h-23.75 w-27.5 rotate-[-22deg] rounded-[64%_36%_43%_57%] bg-blue opacity-[.18] blur-[1px]" />
    </>
  );
}

/**
 * How the repositories compare to their upstreams, as last fetched: shown once all their statuses
 * have loaded, if any has an upstream.
 */
function SyncStatus(props: { summaries: RepositorySummary[] }) {
  const relativeTime = useRelativeTime();
  const sync = createMemo(() => {
    const summaries = props.summaries;
    if (summaries.some((summary) => !summary.settled)) return undefined;
    const tracked = summaries.flatMap(({ status }) => (status?.upstream ? [status] : []));
    if (!tracked.length) return undefined;
    const fetchedAt = Math.max(0, ...summaries.map(({ overview }) => overview?.fetchedAt ?? 0));
    return {
      behind: tracked.filter((status) => status.behind > 0).length,
      ahead: tracked.filter((status) => status.ahead > 0).length,
      fetchedAt: fetchedAt || null,
    };
  });

  return (
    <Show when={sync()}>
      {(state) => {
        const synced = () => !state().behind && !state().ahead;
        return (
          <div class="absolute right-8.75 bottom-6.25 z-2 flex items-center gap-2.5 rounded-[9px] border border-border bg-[color-mix(in_srgb,var(--bg)_82%,transparent)] px-3 py-2.25 backdrop-blur-[10px] max-md:hidden">
            <Show
              when={synced()}
              fallback={
                <span class="grid size-7 place-items-center rounded-full bg-amber-soft text-amber">
                  <ArrowDownUp size={15} />
                </span>
              }
            >
              <span class="grid size-7 place-items-center rounded-full bg-mint-soft text-mint">
                <Check size={16} />
              </span>
            </Show>
            <div class="flex flex-col gap-0.5">
              <strong class="text-[13.5px]">
                {synced() ? "Everything is in sync" : outOfSync(state().behind, state().ahead)}
              </strong>
              <span class="text-[12px] text-muted">
                {state().fetchedAt
                  ? `Last fetched ${relativeTime(state().fetchedAt!).toLowerCase()}`
                  : "Not fetched yet"}
              </span>
            </div>
          </div>
        );
      }}
    </Show>
  );
}

/** How many repositories have commits to pull and to push, e.g. "2 repositories to pull, 1 to push". */
function outOfSync(behind: number, ahead: number): string {
  const counts = [
    [behind, "to pull"],
    [ahead, "to push"],
  ] as const;
  const [first, ...rest] = counts.filter(([count]) => count > 0);
  if (!first) return "";
  const [count, what] = first;
  return [
    `${count} ${count === 1 ? "repository" : "repositories"} ${what}`,
    ...rest.map(([others, which]) => `${others} ${which}`),
  ].join(", ");
}
