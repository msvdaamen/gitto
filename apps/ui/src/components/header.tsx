import { Notifications } from "./notifications";
import { Profile } from "./profile";
import { Tabs } from "./tabs";

export function Header() {
  return (
    <header class="flex min-w-0 items-stretch border-b border-border bg-[color-mix(in_srgb,var(--bg-soft)_92%,var(--primary)_8%)]">
      <Tabs />
      <div class="ml-auto flex min-w-0 items-center gap-1.5 py-0 pr-2.75 pl-2 max-md:pl-0.75">
        <Notifications />
        <Profile />
      </div>
    </header>
  );
}
