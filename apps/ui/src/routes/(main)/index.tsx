import { createFileRoute } from "@tanstack/solid-router";

import { PinnedRepositories } from "@/components/home/pinned-repositories";
import { QuickTip } from "@/components/home/quick-tip";
import { RecentActivity } from "@/components/home/recent-activity";
import { RecentRepositories } from "@/components/home/recent-repositories";
import { WelcomeHero } from "@/components/home/welcome-hero";
import { activities, repositories } from "@/data/mock-data";

export const Route = createFileRoute("/(main)/")({
  component: HomeComponent,
});

const pinned = repositories.filter((repository) => repository.pinned);

// Not wired up yet.
function openRepository() {}

function HomeComponent() {
  return (
    <div class="h-full overflow-auto bg-[radial-gradient(circle_at_17%_-8%,rgba(163,115,215,.1),transparent_31%),var(--bg)] px-[clamp(22px,3vw,46px)] pt-6 pb-10.5 max-[900px]:p-4.5">
      <WelcomeHero onOpenRepository={openRepository} />

      <div class="mx-auto grid max-w-322.5 grid-cols-[minmax(0,1fr)_320px] gap-4.5 max-[1100px]:grid-cols-[minmax(0,1fr)_280px] max-[900px]:grid-cols-1 max-[700px]:block">
        <RecentRepositories repositories={repositories} onOpenRepository={openRepository} />

        <aside class="flex flex-col gap-3 max-[900px]:grid max-[900px]:grid-cols-2 max-[700px]:mt-3.5 max-[700px]:grid-cols-1">
          <PinnedRepositories repositories={pinned} onOpenRepository={openRepository} />
          <RecentActivity activities={activities} />
          <QuickTip />
        </aside>
      </div>
    </div>
  );
}
