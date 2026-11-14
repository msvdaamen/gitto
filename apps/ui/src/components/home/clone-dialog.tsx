import { Dialog } from "@kobalte/core/dialog";
import { cn } from "cn";
import FolderDown from "lucide-solid/icons/folder-down";
import FolderOpen from "lucide-solid/icons/folder-open";
import { createMemo, createSignal, Show } from "solid-js";

import { Button } from "@/components/ui/button";
import { DIALOG_BOX, DialogPortal } from "@/components/ui/dialog";
import { FormField } from "@/components/ui/form-field";
import { inFolder, parentFolder, repositoryName } from "@/git/clone";
import { useCloneRepository, useRepositories, type Repository } from "@/hooks/repositories";
import { rpc } from "@/lib/rpc";

/** Where the folder last chosen to clone into is kept, so it's the one clones go in after a restart. */
const CHOSEN_KEY = "gitto-clone-folder";

function loadChosen(): string | undefined {
  try {
    return localStorage.getItem(CHOSEN_KEY) ?? undefined;
  } catch {
    return undefined;
  }
}

/**
 * Clones a repository from its URL, adds it, and opens it; open while `open`. It goes in a new
 * folder named after it, in the folder chosen with the folder picker: the one last chosen, or until
 * one is, the one the repository added last is in. While it clones, the dialog says how far
 * it's got, and stays open until it's done or cancelled. Why it failed is said in it, with the URL
 * and the folder kept to fix.
 */
export function CloneDialog(props: { open: boolean; onClose: () => void }) {
  const repositories = useRepositories();
  const cloning = useCloneRepository();
  const [url, setUrl] = createSignal("");
  /** The folder chosen with the picker, which clones go in from then on. */
  const [chosen, setChosen] = createSignal(loadChosen());
  const parent = createMemo(() => {
    // Ids are UUIDv7s, which sort by when they were made: the greatest is the one added last.
    const newest = repositories.data?.reduce<Repository | undefined>(
      (latest, repository) => (!latest || repository.id > latest.id ? repository : latest),
      undefined,
    );
    return chosen() ?? (newest && parentFolder(newest.path));
  });
  const name = () => repositoryName(url());
  /** The folder it's cloned into, once there's a URL to name it after and a folder to put it in. */
  const destination = () => {
    const into = parent();
    return into && name() ? inFolder(into, name()) : undefined;
  };

  async function choose() {
    const folder = await rpc.system.selectFolder();
    if (!folder) return;
    setChosen(folder);
    try {
      localStorage.setItem(CHOSEN_KEY, folder);
    } catch {
      // Unavailable: it's still used until Gitto closes.
    }
    cloning.dismiss();
  }

  function submit(event: Event) {
    event.preventDefault();
    const path = destination();
    if (!path || cloning.running()) return;
    void cloning.clone(url().trim(), path, () => {
      props.onClose();
      setUrl("");
    });
  }

  function close() {
    // Only Cancel stops a clone: it's not given up on by a click beside the dialog.
    if (cloning.running()) return;
    cloning.dismiss();
    props.onClose();
  }

  const percent = () => cloning.progress()?.percent ?? 0;

  return (
    <Dialog open={props.open} onOpenChange={(open) => !open && close()} modal>
      <DialogPortal>
        <Dialog.Content class={cn(DIALOG_BOX, "max-w-[440px] p-4")}>
          <form onSubmit={submit}>
            <Dialog.Title class="m-0 text-[12.5px] font-[680]">Clone repository</Dialog.Title>
            <Dialog.Description class="m-0 mt-1 mb-2.5 text-[11.5px] leading-[1.45] text-muted">
              Copies a repository and its history into a folder of its own, and opens it.
            </Dialog.Description>
            <FormField
              label="URL"
              placeholder="https://github.com/owner/repository.git"
              value={url()}
              onChange={(value) => {
                setUrl(value);
                cloning.dismiss();
              }}
              readOnly={cloning.running()}
            />
            <div class="mb-2.5 flex flex-col gap-[5px] text-[11.5px] text-muted">
              <span id="clone-location" class="flex justify-between">
                Clone into
                <small class="text-faint">A new folder, named after the repository</small>
              </span>
              <button
                type="button"
                aria-labelledby="clone-location"
                disabled={cloning.running()}
                title={destination() ?? parent()}
                class="flex w-full cursor-pointer items-center gap-2 rounded-md border border-border bg-bg px-[9px] py-2 text-left text-[12px] text-text outline-0 focus-visible:border-primary focus-visible:shadow-[0_0_0_3px_var(--primary-soft)] enabled:hover:border-[color-mix(in_srgb,var(--primary)_45%,var(--border))] disabled:cursor-default disabled:opacity-60"
                onClick={() => void choose()}
              >
                <FolderOpen size={14} class="shrink-0 text-muted" />
                <span class="min-w-0 flex-1 truncate">
                  <Show
                    when={parent()}
                    fallback={<span class="text-faint">Choose a folder to clone into</span>}
                  >
                    {(into) => (
                      <>
                        <span class={name() ? "text-muted" : undefined}>
                          {name() ? inFolder(into(), "") : into()}
                        </span>
                        {name()}
                      </>
                    )}
                  </Show>
                </span>
                <span class="shrink-0 text-primary-strong">Choose…</span>
              </button>
            </div>
            <Show when={cloning.running()}>
              <div class="mb-2.5 flex flex-col gap-[5px] text-[11.5px] text-muted" role="status">
                <div class="flex justify-between">
                  <span>
                    {cloning.cancellable()
                      ? (cloning.progress()?.phase ?? "Connecting")
                      : "Opening"}
                    …
                  </span>
                  <Show when={cloning.progress()}>
                    <span class="tabular-nums">{percent()}%</span>
                  </Show>
                </div>
                <div
                  class="h-1 overflow-hidden rounded-full bg-panel-raised"
                  role="progressbar"
                  aria-valuemin={0}
                  aria-valuemax={100}
                  aria-valuenow={percent()}
                  aria-label="Cloning"
                >
                  <div
                    class="h-full rounded-full bg-primary transition-[width] duration-200 motion-reduce:transition-none"
                    style={{ width: `${percent()}%` }}
                  />
                </div>
              </div>
            </Show>
            <Show when={cloning.error()}>
              {(error) => (
                <p
                  role="alert"
                  class="m-0 mb-2.5 text-[11.5px] leading-[1.45] break-words whitespace-pre-line text-coral"
                >
                  {error().message}
                </p>
              )}
            </Show>
            <div class="flex gap-2">
              <Show when={cloning.running()}>
                <Button
                  type="button"
                  variant="secondary"
                  class="h-8 shrink-0 rounded-[7px] text-[12.5px] shadow-none"
                  disabled={!cloning.cancellable()}
                  onClick={cloning.cancel}
                >
                  Cancel
                </Button>
              </Show>
              <Button
                type="submit"
                variant="primary"
                icon={FolderDown}
                disabled={!destination() || cloning.running()}
                class="h-8 flex-1 gap-[7px] rounded-[7px] text-[12.5px] font-[680] shadow-none"
              >
                {cloning.running() ? "Cloning…" : "Clone"}
              </Button>
            </div>
          </form>
        </Dialog.Content>
      </DialogPortal>
    </Dialog>
  );
}
