import Bell from "lucide-solid/icons/bell";
import Moon from "lucide-solid/icons/moon";
import Search from "lucide-solid/icons/search";
import Sun from "lucide-solid/icons/sun";

import { useTheme } from "@/hooks/theme";

import { Profile } from "./profile";
import { Tabs } from "./tabs";
import { IconButton } from "./ui/button";
import { TextInput } from "./ui/text-input";

export function Header() {
  const { theme, toggleTheme } = useTheme();
  return (
    <header class="flex min-w-0 items-stretch border-b border-border bg-[color-mix(in_srgb,var(--bg-soft)_92%,var(--primary)_8%)]">
      <Tabs />
      <div class="ml-auto flex min-w-0 items-center gap-1.5 py-0 pr-2.75 pl-2 max-md:pl-0.75 max-sm:[&>button:not(:first-of-type)]:hidden">
        <TextInput icon={Search} placeholder="Search commands" compact />
        <IconButton
          label={`Switch to ${theme() === "dark" ? "light" : "dark"} theme`}
          icon={theme() === "dark" ? Sun : Moon}
          onClick={toggleTheme}
        />
        <span class="relative flex max-sm:hidden">
          <IconButton label="Notifications" icon={Bell} />
          <i class="pointer-events-none absolute top-1.25 right-1.25 size-1.25 rounded-full bg-coral shadow-[0_0_0_2px_var(--bg-soft)]" />
        </span>
        <Profile />
      </div>
    </header>
  );
}
