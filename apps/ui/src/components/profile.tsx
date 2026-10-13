import { DropdownMenu } from "@kobalte/core/dropdown-menu";
import { useNavigate } from "@tanstack/solid-router";
import type { LucideIcon } from "lucide-solid";
import Moon from "lucide-solid/icons/moon";
import Settings from "lucide-solid/icons/settings";
import Sun from "lucide-solid/icons/sun";
import { Dynamic } from "solid-js/web";

import { useTheme } from "@/hooks/theme";

/** The user's avatar, which opens a menu of what's Gitto's own rather than a repository's. */
export function Profile() {
  const navigate = useNavigate();
  const { theme, toggleTheme } = useTheme();

  return (
    <DropdownMenu>
      <DropdownMenu.Trigger
        class="size-7 cursor-pointer rounded-full border border-[color-mix(in_srgb,var(--primary)_45%,var(--border))] bg-[linear-gradient(145deg,#9770d1,#557fc8)] p-0.5 focus-ring"
        aria-label="Open profile"
      >
        <span class="grid size-full place-items-center rounded-full bg-[rgba(20,16,25,.35)] text-[11.5px] font-bold text-white">
          MV
        </span>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content class="z-50 min-w-[180px] animate-toast-in rounded-lg border border-border bg-panel-raised p-1 text-text shadow-app outline-none motion-reduce:animate-none">
          <ProfileMenuItem
            icon={theme() === "dark" ? Sun : Moon}
            label={theme() === "dark" ? "Light theme" : "Dark theme"}
            onSelect={toggleTheme}
          />
          <ProfileMenuItem
            icon={Settings}
            label="Settings"
            onSelect={() => void navigate({ to: "/settings" })}
          />
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu>
  );
}

function ProfileMenuItem(props: { icon: LucideIcon; label: string; onSelect: () => void }) {
  return (
    <DropdownMenu.Item
      class="flex h-7 cursor-pointer items-center gap-2 rounded-[5px] px-2 text-[12.5px] text-text-soft outline-none data-highlighted:bg-panel-hover data-highlighted:text-text"
      // Read on each choice: Kobalte binds a handler once.
      onSelect={() => props.onSelect()}
    >
      <Dynamic component={props.icon} size={13} class="shrink-0" />
      <DropdownMenu.ItemLabel class="truncate">{props.label}</DropdownMenu.ItemLabel>
    </DropdownMenu.Item>
  );
}
