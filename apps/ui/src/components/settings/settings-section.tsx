import type { LucideIcon } from "lucide-solid";
import type { JSX } from "solid-js";
import { Dynamic } from "solid-js/web";

/**
 * A group of settings on the settings page, like Privacy: its title, what it's about, and its
 * settings, each in a row of its own.
 */
export function SettingsSection(props: {
  icon: LucideIcon;
  title: string;
  description: string;
  children: JSX.Element;
}) {
  return (
    <section class="rounded-[11px] border border-border bg-panel p-5">
      <div class="flex items-start gap-3">
        <span class="grid size-8 shrink-0 place-items-center rounded-[9px] bg-primary-soft text-primary-strong">
          <Dynamic component={props.icon} size={16} strokeWidth={1.9} />
        </span>
        <div class="min-w-0">
          <h2 class="m-0 text-[15px] tracking-[-.2px]">{props.title}</h2>
          <p class="m-0 mt-0.5 text-[13px] text-muted">{props.description}</p>
        </div>
      </div>
      <div class="mt-4 flex flex-col divide-y divide-border-soft border-t border-border-soft [&>*]:py-4 [&>*:last-child]:pb-0">
        {props.children}
      </div>
    </section>
  );
}
