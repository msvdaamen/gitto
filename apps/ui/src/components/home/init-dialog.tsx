import { Dialog } from "@kobalte/core/dialog";
import { cn } from "cn";
import FolderPlus from "lucide-solid/icons/folder-plus";
import { createSignal, Show } from "solid-js";

import { Button } from "@/components/ui/button";
import { DIALOG_BOX, DialogPortal } from "@/components/ui/dialog";
import { FormField } from "@/components/ui/form-field";
import { inFolder } from "@/git/clone";
import { folderName } from "@/git/init";
import { useInitRepository } from "@/hooks/repositories";

import { LocationField, useLocation } from "./location-field";

/**
 * Creates an empty repository, adds it, and opens it; open while `open`. It goes in a folder named
 * after it, in the folder chosen with the folder picker: at first the one the last repository is
 * in, and the one last chosen from then on. A folder that's there already is made a repository,
 * keeping what's in it. Why it failed is said in it, with the name and folder kept to fix.
 */
export function InitDialog(props: { open: boolean; onClose: () => void }) {
  const init = useInitRepository();
  const location = useLocation();
  const [name, setName] = createSignal("");
  const folder = () => folderName(name());
  /** Where it's created, once there's a name for its folder and a folder to put it in. */
  const destination = () => {
    const into = location.parent();
    return into && folder() ? inFolder(into, folder()) : undefined;
  };

  async function choose() {
    if (await location.choose()) init.reset();
  }

  function submit(event: Event) {
    event.preventDefault();
    const path = destination();
    if (!path || init.isPending) return;
    init.mutate(path, {
      onSuccess: () => {
        props.onClose();
        setName("");
      },
    });
  }

  function close() {
    init.reset();
    props.onClose();
  }

  return (
    <Dialog open={props.open} onOpenChange={(open) => !open && close()} modal>
      <DialogPortal>
        <Dialog.Content class={cn(DIALOG_BOX, "max-w-[440px] p-4")}>
          <form onSubmit={submit}>
            <Dialog.Title class="m-0 text-[12.5px] font-[680]">New repository</Dialog.Title>
            <Dialog.Description class="m-0 mt-1 mb-2.5 text-[11.5px] leading-[1.45] text-muted">
              Creates an empty repository in a folder of its own, and opens it. In a folder that's
              there already, what's in it is kept, ready to commit.
            </Dialog.Description>
            <FormField
              label="Name"
              placeholder="my-project"
              value={name()}
              onChange={(value) => {
                setName(value);
                init.reset();
              }}
              readOnly={init.isPending}
            />
            <LocationField
              label="Create in"
              hint="A folder named after the repository"
              placeholder="Choose a folder to create it in"
              parent={location.parent()}
              name={folder()}
              disabled={init.isPending}
              onChoose={() => void choose()}
            />
            <Show when={init.error}>
              {(error) => (
                <p
                  role="alert"
                  class="m-0 mb-2.5 text-[11.5px] leading-[1.45] break-words whitespace-pre-line text-coral"
                >
                  {error().message}
                </p>
              )}
            </Show>
            <Button
              type="submit"
              variant="primary"
              icon={FolderPlus}
              disabled={!destination() || init.isPending}
              class="h-8 w-full gap-[7px] rounded-[7px] text-[12.5px] font-[680] shadow-none"
            >
              {init.isPending ? "Creating…" : "Create repository"}
            </Button>
          </form>
        </Dialog.Content>
      </DialogPortal>
    </Dialog>
  );
}
