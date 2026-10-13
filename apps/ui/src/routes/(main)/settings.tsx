import { createFileRoute } from "@tanstack/solid-router";
import ShieldCheck from "lucide-solid/icons/shield-check";

import { SettingsSection } from "@/components/settings/settings-section";
import { TelemetrySetting } from "@/components/settings/telemetry-setting";

export const Route = createFileRoute("/(main)/settings")({
  component: SettingsComponent,
});

/** Gitto's settings, a section for each kind. */
function SettingsComponent() {
  return (
    <div class="h-full overflow-auto bg-[radial-gradient(circle_at_17%_-8%,rgba(163,115,215,.1),transparent_31%),var(--bg)] px-[clamp(22px,3vw,46px)] pt-6 pb-10.5 max-md:p-4.5">
      <div class="mx-auto flex max-w-180 flex-col gap-4">
        <div>
          <h1 class="m-0 text-[22px] tracking-[-.4px]">Settings</h1>
          <p class="m-0 mt-1 text-[13px] text-muted">They apply to Gitto on this computer.</p>
        </div>

        <SettingsSection
          icon={ShieldCheck}
          title="Privacy"
          description="What Gitto sends, and what it keeps to itself."
        >
          <TelemetrySetting />
        </SettingsSection>
      </div>
    </div>
  );
}
