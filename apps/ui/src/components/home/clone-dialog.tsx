import { Dialog } from "@kobalte/core/dialog";
import { cn } from "cn";
import FolderDown from "lucide-solid/icons/folder-down";
import { createSignal, Show } from "solid-js";

import { Button } from "@/components/ui/button";
import { DIALOG_BOX, DialogPortal } from "@/components/ui/dialog";
import { FormField } from "@/components/ui/form-field";
import { inFolder, repositoryName } from "@/git/clone";
import { useCloneRepository } from "@/hooks/repositories";

import { LocationField, useLocation } from "./location-field";

/**
 * Clones a repository from its URL, adds it, and opens it; open while `open`. It goes in a new
 * folder named after it, in the folder chosen with the folder picker: at first the one the last
 * repository is in, and the one last chosen from then on. While it clones, the dialog says how far
 * it's got, and stays open until it's done or cancelled. Why it failed is said in it, with the URL
 * and the folder kept to fix.
 */
export function CloneDialog(props: { open: boolean; onClose: () => void }) {
  const cloning = useCloneRepository();
  const location = useLocation();
  const [url, setUrl] = createSignal("");
  const name = () => repositoryName(url());
  /** The folder it's cloned into, once there's a URL to name it after and a folder to put it in. */
  const destination = () => {
    const into = location.parent();
    return into && name() ? inFolder(into, name()) : undefined;
  };

  async function choose() {
    if (await location.choose()) cloning.dismiss();
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
            <LocationField
              label="Clone into"
              hint="A new folder, named after the repository"
              placeholder="Choose a folder to clone into"
              parent={location.parent()}
              name={name()}
              disabled={cloning.running()}
              onChoose={() => void choose()}
            />
            <Show when={cloning.running()}>
              <div class="mb-2.5 flex flex-col gap-[5px] text-[11.5px] text-muted" role="status">
                <div class="flex justify-between">
                  <span>{cloning.progress()?.phase ?? "Connecting"}…</span>
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
