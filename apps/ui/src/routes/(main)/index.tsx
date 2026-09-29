import { createFileRoute } from "@tanstack/solid-router";
import Check from "lucide-solid/icons/check";
import ChevronRight from "lucide-solid/icons/chevron-right";
import Command from "lucide-solid/icons/command";
import Copy from "lucide-solid/icons/copy";
import Ellipsis from "lucide-solid/icons/ellipsis";
import Folder from "lucide-solid/icons/folder";
import GitBranch from "lucide-solid/icons/git-branch";
import GitMerge from "lucide-solid/icons/git-merge";
import Pin from "lucide-solid/icons/pin";
import Plus from "lucide-solid/icons/plus";
import RotateCcwClock from "lucide-solid/icons/rotate-ccw-clock";
import Search from "lucide-solid/icons/search";
import Sparkles from "lucide-solid/icons/sparkles";
import Upload from "lucide-solid/icons/upload";
import { createMemo, createSignal, For, Show } from "solid-js";
import { Dynamic } from "solid-js/web";

import { StatusDot } from "@/components/ui/status-dot";
import { activities, repositories } from "@/data/mock-data";

import { Badge } from "../../components/ui/badge";
import { Button, IconButton } from "../../components/ui/button";
import { TextInput } from "../../components/ui/text-input";

export const Route = createFileRoute("/(main)/")({
  component: HomeComponent,
});

function HomeComponent() {
  const [query, setQuery] = createSignal("");
  const filtered = createMemo(() => {
    const needle = query().trim().toLowerCase();
    if (!needle) return repositories;
    return repositories.filter((repository) =>
      `${repository.name} ${repository.owner} ${repository.path} ${repository.branch}`
        .toLowerCase()
        .includes(needle),
    );
  });
  const pinned = createMemo(() => repositories.filter((repository) => repository.pinned));

  function onDemoAction(message: string) {
    alert(message);
  }
  function onOpenRepository() {}

  const activityTones = {
    purple: "bg-primary-soft text-primary",
    blue: "bg-blue-soft text-blue",
    mint: "bg-mint-soft text-mint",
    amber: "bg-amber-soft text-amber",
  };

  return (
    <div class="h-full overflow-auto bg-[radial-gradient(circle_at_17%_-8%,rgba(163,115,215,.1),transparent_31%),var(--bg)] px-[clamp(22px,3vw,46px)] pt-6 pb-10.5 max-[900px]:p-4.5">
      <section class="relative mx-auto mb-5.5 flex min-h-52.5 max-w-322.5 items-center overflow-hidden rounded-[13px] border border-[color-mix(in_srgb,var(--primary)_30%,var(--border))] bg-[linear-gradient(122deg,color-mix(in_srgb,var(--panel)_90%,var(--primary)_10%),var(--panel))] px-9.5 py-8 shadow-app after:pointer-events-none after:absolute after:inset-0 after:bg-[radial-gradient(color-mix(in_srgb,var(--primary)_55%,transparent)_.65px,transparent_.65px)] after:bg-size-[17px_17px] after:opacity-[.18] after:mask-[linear-gradient(90deg,transparent,#000)] after:content-[''] max-[700px]:min-h-57.5 max-[700px]:p-6.5">
        <div class="pointer-events-none absolute -top-10.75 right-27.5 h-38 w-45 rotate-12 rounded-[44%_56%_62%_38%/47%_38%_62%_53%] bg-[linear-gradient(145deg,var(--primary),#5c62d5)] opacity-30 blur-[.2px] before:absolute before:top-18 before:left-15.75 before:size-1.75 before:rounded-full before:bg-[rgba(30,21,37,.8)] before:shadow-[31px_0_rgba(30,21,37,.8)] before:content-[''] max-[900px]:right-2.5 [&>span:first-child]:absolute [&>span:first-child]:top-23.5 [&>span:first-child]:left-18.5 [&>span:first-child]:h-1.75 [&>span:first-child]:w-5.25 [&>span:first-child]:rounded-b-[50%] [&>span:first-child]:border-b-2 [&>span:first-child]:border-[rgba(30,21,37,.7)]">
          <span />
          <span />
        </div>
        <div class="pointer-events-none absolute -right-7 -bottom-9.5 h-23.75 w-27.5 rotate-[-22deg] rounded-[64%_36%_43%_57%] bg-blue opacity-[.18] blur-[1px]" />
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
          <div class="flex items-center gap-2 max-[700px]:flex-wrap">
            <Button variant="primary" icon={Folder} onClick={onOpenRepository}>
              Open repository
            </Button>
            <Button
              icon={Copy}
              onClick={() => onDemoAction("Clone repository is ready for Git integration.")}
            >
              Clone from URL
            </Button>
            <Button
              variant="ghost"
              icon={Plus}
              onClick={() => onDemoAction("Initialize repository is a demo action.")}
            >
              New repository
            </Button>
          </div>
        </div>
        <div class="absolute right-8.75 bottom-6.25 z-2 flex items-center gap-2.5 rounded-[9px] border border-border bg-[color-mix(in_srgb,var(--bg)_82%,transparent)] px-3 py-2.25 backdrop-blur-[10px] max-[900px]:hidden">
          <span class="grid size-7 place-items-center rounded-full bg-mint-soft text-mint">
            <Check size={16} />
          </span>
          <div class="flex flex-col gap-0.5">
            <strong class="text-[11px]">Everything is in sync</strong>
            <span class="text-[9.5px] text-muted">Last fetched 2 minutes ago</span>
          </div>
        </div>
      </section>

      <div class="mx-auto grid max-w-322.5 grid-cols-[minmax(0,1fr)_320px] gap-4.5 max-[1100px]:grid-cols-[minmax(0,1fr)_280px] max-[900px]:grid-cols-1 max-[700px]:block">
        <section class="rounded-[11px] border border-border bg-panel p-5">
          <div class="mb-4 flex items-center justify-between gap-4 max-[700px]:flex-col max-[700px]:items-start max-[700px]:[&>label]:w-full">
            <div>
              <h2 class="mt-0 mb-0.75 text-[15px] tracking-[-.2px]">Recent repositories</h2>
              <p class="m-0 text-[10.5px] text-muted">Your latest local workspaces</p>
            </div>
            <TextInput
              icon={Search}
              value={query()}
              onChange={setQuery}
              placeholder="Find a repository…"
              compact
            />
          </div>
          <div class="flex flex-col gap-1.5">
            <Show
              when={filtered().length}
              fallback={
                <div class="grid h-40 place-items-center content-center gap-2 text-faint">
                  <Search size={22} />
                  <p class="m-0">No repositories match “{query()}”.</p>
                </div>
              }
            >
              <For each={filtered()}>
                {(repository) => (
                  <button
                    class="group grid min-h-20.5 w-full cursor-pointer grid-cols-[42px_minmax(0,1fr)_auto_24px] items-center gap-3 rounded-[9px] border border-transparent bg-transparent py-2.75 pr-2.75 pl-3 text-left text-text [-webkit-tap-highlight-color:transparent] hover:border-border hover:bg-panel-hover focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary max-[700px]:grid-cols-[38px_minmax(0,1fr)_18px]"
                    onClick={onOpenRepository}
                    data-repository={repository.id}
                  >
                    <span class="grid size-10 place-items-center rounded-[10px] bg-[linear-gradient(145deg,var(--primary-soft),var(--blue-soft))] text-primary-strong shadow-[inset_0_0_0_1px_color-mix(in_srgb,var(--primary)_18%,transparent)]">
                      {repository.provider === "Local" ? (
                        <Folder size={19} />
                      ) : (
                        <GitBranch size={19} />
                      )}
                    </span>
                    <span class="flex min-w-0 flex-col gap-1.2">
                      <span class="flex items-center gap-2">
                        <strong class="text-[13px]">{repository.name}</strong>
                        {repository.pinned && <Pin class="text-amber" size={12} />}
                        <Badge
                          tone={
                            repository.provider === "GitHub"
                              ? "purple"
                              : repository.provider === "GitLab"
                                ? "blue"
                                : "neutral"
                          }
                        >
                          {repository.provider}
                        </Badge>
                      </span>
                      <span class="truncate text-[10.5px] text-muted">
                        {repository.description}
                      </span>
                      <span class="flex items-center gap-3.25 text-[9.5px] text-muted [&>span]:flex [&>span]:items-center [&>span]:gap-1.25 [&>span]:whitespace-nowrap">
                        <span>
                          <GitBranch size={13} />
                          {repository.branch}
                        </span>
                        <span>
                          <StatusDot color={repository.languageColor} />
                          {repository.language}
                        </span>
                        <Show
                          when={
                            repository.status.modified +
                              repository.status.added +
                              repository.status.deleted >
                            0
                          }
                          fallback={
                            <span class="text-mint">
                              <Check size={12} />
                              Clean
                            </span>
                          }
                        >
                          <span class="text-amber">
                            {repository.status.modified +
                              repository.status.added +
                              repository.status.deleted}{" "}
                            changes
                          </span>
                        </Show>
                      </span>
                    </span>
                    <span class="self-start pt-1 text-[9.5px] whitespace-nowrap text-faint max-[700px]:hidden">
                      {repository.lastOpened}
                    </span>
                    <span class="grid -translate-x-0.75 place-items-center text-faint opacity-0 transition-[opacity,transform] duration-150 group-hover:translate-x-0 group-hover:opacity-100 motion-reduce:transition-none">
                      <ChevronRight size={16} />
                    </span>
                  </button>
                )}
              </For>
            </Show>
          </div>
        </section>

        <aside class="flex flex-col gap-3 max-[900px]:grid max-[900px]:grid-cols-2 max-[700px]:mt-3.5 max-[700px]:grid-cols-1">
          <section class="rounded-[11px] border border-border bg-panel p-3">
            <div class="mb-1.25 flex h-7 items-center justify-between">
              <div class="flex items-center gap-1.75 [&>svg]:text-primary">
                <Pin size={15} />
                <h3 class="m-0 text-[11.5px]">Pinned</h3>
              </div>
              <IconButton label="Manage pinned repositories" icon={Ellipsis} />
            </div>
            <For each={pinned()}>
              {(repository) => (
                <button
                  class="grid w-full cursor-pointer grid-cols-[8px_minmax(0,1fr)_auto] items-center gap-2.25 rounded-[7px] border-0 bg-transparent p-2 text-left hover:bg-panel-hover [&>svg]:text-faint"
                  onClick={onOpenRepository}
                >
                  <span class="size-1.5 rounded-full bg-primary shadow-[0_0_8px_color-mix(in_srgb,var(--primary)_55%,transparent)]" />
                  <span class="flex min-w-0 flex-col gap-0.5">
                    <strong class="text-[10.5px]">{repository.name}</strong>
                    <small class="text-[9px] text-faint">{repository.owner}</small>
                  </span>
                  <ChevronRight size={14} />
                </button>
              )}
            </For>
          </section>
          <section class="rounded-[11px] border border-border bg-panel p-3">
            <div class="mb-1.25 flex h-7 items-center justify-between">
              <div class="flex items-center gap-1.75 [&>svg]:text-primary">
                <RotateCcwClock size={15} />
                <h3 class="m-0 text-[11.5px]">Recent activity</h3>
              </div>
              <button class="cursor-pointer border-0 bg-transparent text-[9.5px] text-primary-strong">
                View all
              </button>
            </div>
            <div class="flex flex-col">
              <For each={activities}>
                {(activity) => (
                  <div class="grid grid-cols-[27px_minmax(0,1fr)_auto] items-center gap-2 border-t border-border-soft px-1.25 py-2 first:border-t-0">
                    <span
                      class={`grid size-6.25 place-items-center rounded-[7px] ${activityTones[activity.tone]}`}
                    >
                      <Dynamic
                        component={
                          activity.action.startsWith("Merged")
                            ? GitMerge
                            : activity.action.startsWith("Created")
                              ? GitBranch
                              : Upload
                        }
                        size={13}
                      />
                    </span>
                    <div class="flex min-w-0 flex-col gap-0.5">
                      <strong class="truncate text-[9.5px]">{activity.action}</strong>
                      <span class="text-[8.5px] text-faint">
                        {activity.repository} · {activity.branch}
                      </span>
                    </div>
                    <time class="text-[8.5px] whitespace-nowrap text-faint">{activity.time}</time>
                  </div>
                )}
              </For>
            </div>
          </section>
          <button
            class="col-span-full flex w-full cursor-pointer items-start gap-2.5 rounded-[10px] border border-dashed border-[color-mix(in_srgb,var(--primary)_38%,var(--border))] bg-primary-soft p-3 text-left text-text"
            onClick={() => onDemoAction("Command palette is coming next.")}
          >
            <span class="grid size-7 place-items-center rounded-[7px] bg-panel text-primary-strong">
              <Command size={17} />
            </span>
            <div class="flex-1">
              <strong class="text-[10.5px]">Quick tip</strong>
              <p class="mt-1 mb-0 text-[9.5px] leading-1.5 text-muted">
                Press{" "}
                <kbd class="rounded-sm border border-border bg-panel-raised px-1.25 py-0.5 font-mono text-[10px] text-faint">
                  ⌘ K
                </kbd>{" "}
                to open the command palette.
              </p>
            </div>
          </button>
        </aside>
      </div>
    </div>
  );
}
