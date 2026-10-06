import { createFileRoute } from "@tanstack/solid-router";

import { RecentActivity } from "@/components/home/recent-activity";
import { RepositoryList } from "@/components/home/repository-list";
import { useRepositorySummaries } from "@/components/home/summaries";
import { WelcomeHero } from "@/components/home/welcome-hero";
import { useUnsuspendedData } from "@/git/queries/unsuspended";
import { useUserName } from "@/git/queries/user";
import { useOpenRepository, useRepositories } from "@/hooks/repositories";

export const Route = createFileRoute("/(main)/")({
  component: HomeComponent,
});

function HomeComponent() {
  // Read without Suspense, like the repositories' summaries: the page shows straight away, and
  // fills in as they load.
  const repositories = useUnsuspendedData(useRepositories());
  const summaries = useRepositorySummaries(() => repositories() ?? []);
  const userName = useUnsuspendedData(useUserName());
  const openRepository = useOpenRepository();

  return (
    <div class="h-full overflow-auto bg-[radial-gradient(circle_at_17%_-8%,rgba(163,115,215,.1),transparent_31%),var(--bg)] px-[clamp(22px,3vw,46px)] pt-6 pb-10.5 max-md:p-4.5">
      <WelcomeHero
        userName={userName()}
        summaries={summaries()}
        onOpenRepository={openRepository.open}
        openError={openRepository.error()}
        onDismissOpenError={openRepository.dismiss}
      />

      <div class="mx-auto grid max-w-322.5 grid-cols-[minmax(0,1fr)_320px] gap-4.5 max-lg:grid-cols-[minmax(0,1fr)_280px] max-md:grid-cols-1 max-sm:block">
        <RepositoryList
          summaries={summaries()}
          loaded={repositories() !== undefined}
          onOpenRepository={openRepository.open}
        />

        <aside class="flex flex-col gap-3 max-sm:mt-3.5">
          <RecentActivity summaries={summaries()} />
        </aside>
      </div>
    </div>
  );
}
