import FolderOpen from "lucide-solid/icons/folder-open";
import { createMemo, createSignal, createUniqueId, Show } from "solid-js";

import { inFolder, parentFolder } from "@/git/clone";
import { useRepositories } from "@/hooks/repositories";
import { rpc } from "@/lib/rpc";

/**
 * The folder a new repository goes in, as a folder of its own: at first the one the last
 * repository is in, and the one last chosen with the folder picker from then on.
 */
export function useLocation() {
  const repositories = useRepositories();
  /** The folder chosen with the picker, which new repositories go in from then on. */
  const [chosen, setChosen] = createSignal<string>();
  const parent = createMemo(() => {
    const last = repositories.data?.at(-1);
    return chosen() ?? (last && parentFolder(last.path));
  });
  return {
    /** The folder it goes in, once there's one to suggest or one was chosen. */
    parent,
    /** Asks for the folder to put it in; resolves to whether one was chosen. */
    choose: async () => {
      const folder = await rpc.system.selectFolder();
      if (folder) setChosen(folder);
      return !!folder;
    },
  };
}

/**
 * A button showing where a new repository named `name` goes, its folder in `parent`, which opens
 * the folder picker to choose another `parent`.
 */
export function LocationField(props: {
  label: string;
  hint: string;
  /** Said until there's a folder to put it in. */
  placeholder: string;
  parent: string | undefined;
  /** Its folder's name, once there's one. */
  name: string;
  disabled?: boolean;
  onChoose: () => void;
}) {
  const id = createUniqueId();
  return (
    <div class="mb-2.5 flex flex-col gap-[5px] text-[11.5px] text-muted">
      <span id={id} class="flex justify-between">
        {props.label}
        <small class="text-faint">{props.hint}</small>
      </span>
      <button
        type="button"
        aria-labelledby={id}
        disabled={props.disabled}
        title={props.parent && props.name ? inFolder(props.parent, props.name) : props.parent}
        class="flex w-full cursor-pointer items-center gap-2 rounded-md border border-border bg-bg px-[9px] py-2 text-left text-[12px] text-text outline-0 focus-visible:border-primary focus-visible:shadow-[0_0_0_3px_var(--primary-soft)] enabled:hover:border-[color-mix(in_srgb,var(--primary)_45%,var(--border))] disabled:cursor-default disabled:opacity-60"
        onClick={() => props.onChoose()}
      >
        <FolderOpen size={14} class="shrink-0 text-muted" />
        <span class="min-w-0 flex-1 truncate">
          <Show when={props.parent} fallback={<span class="text-faint">{props.placeholder}</span>}>
            {(into) => (
              <>
                <span class={props.name ? "text-muted" : undefined}>
                  {props.name ? inFolder(into(), "") : into()}
                </span>
                {props.name}
              </>
            )}
          </Show>
        </span>
        <span class="shrink-0 text-primary-strong">Choose…</span>
      </button>
    </div>
  );
}
