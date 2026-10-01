import Check from "lucide-solid/icons/check";
import Copy from "lucide-solid/icons/copy";
import Folder from "lucide-solid/icons/folder";
import Plus from "lucide-solid/icons/plus";
import Sparkles from "lucide-solid/icons/sparkles";

import { Button } from "@/components/ui/button";

import { demoAction } from "./demo-action";

/** The home page banner: a greeting and ways to open or create a repository. */
export function WelcomeHero(props: { onOpenRepository: () => void }) {
  return (
    <section class="relative mx-auto mb-5.5 flex min-h-52.5 max-w-322.5 items-center overflow-hidden rounded-[13px] border border-[color-mix(in_srgb,var(--primary)_30%,var(--border))] bg-[linear-gradient(122deg,color-mix(in_srgb,var(--panel)_90%,var(--primary)_10%),var(--panel))] px-9.5 py-8 shadow-app after:pointer-events-none after:absolute after:inset-0 after:bg-[radial-gradient(color-mix(in_srgb,var(--primary)_55%,transparent)_.65px,transparent_.65px)] after:bg-size-[17px_17px] after:opacity-[.18] after:mask-[linear-gradient(90deg,transparent,#000)] after:content-[''] max-sm:min-h-57.5 max-sm:p-6.5">
      <HeroDecoration />
      <div class="relative z-2">
        <div class="mb-3 flex items-center gap-1.75 text-[10px] font-[720] tracking-[.11em] text-primary-strong uppercase">
          <span class="grid size-5.75 place-items-center rounded-[7px] bg-primary-soft text-primary-strong">
            <Sparkles size={13} />
          </span>{" "}
          Your workspace
        </div>
        <h1 class="m-0 text-[clamp(27px,2.6vw,38px)] leading-[1.1] tracking-[-1.25px]">
          Welcome back, Mischa.
        </h1>
        <p class="mt-2.25 mb-5 text-sm text-muted">
          Pick up where you left off, or shape something new.
        </p>
        <div class="flex items-center gap-2 max-sm:flex-wrap">
          <Button variant="primary" icon={Folder} onClick={props.onOpenRepository}>
            Open repository
          </Button>
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
      <SyncStatus />
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

function SyncStatus() {
  return (
    <div class="absolute right-8.75 bottom-6.25 z-2 flex items-center gap-2.5 rounded-[9px] border border-border bg-[color-mix(in_srgb,var(--bg)_82%,transparent)] px-3 py-2.25 backdrop-blur-[10px] max-md:hidden">
      <span class="grid size-7 place-items-center rounded-full bg-mint-soft text-mint">
        <Check size={16} />
      </span>
      <div class="flex flex-col gap-0.5">
        <strong class="text-[11px]">Everything is in sync</strong>
        <span class="text-[9.5px] text-muted">Last fetched 2 minutes ago</span>
      </div>
    </div>
  );
}
