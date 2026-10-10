import { Dialog } from "@kobalte/core/dialog";
import { cn } from "cn";
import FolderPlus from "lucide-solid/icons/folder-plus";
import { createEffect, createMemo, createSignal, on, Show } from "solid-js";

import { Button } from "@/components/ui/button";
import { DIALOG_BOX, DialogPortal } from "@/components/ui/dialog";
import { FormField } from "@/components/ui/form-field";
import { useRefs } from "@/git/queries/refs";
import { useWorktreeActions, useWorktrees } from "@/git/queries/worktree";
import { branchName } from "@/git/status";
import { checkedOutIn, suggestWorktreeFolder } from "@/git/worktree";
import { useRepository } from "@/hooks/repositories";
import { rpc } from "@/lib/rpc";

/**
 * Places a new worktree for the branch `branch`, by its full ref name, and adds it; open while
 * there's a `branch`. The folder is suggested beside the repository, named after it and the
 * branch, and can be typed or picked. The branch is checked out there (for a remote one, the
 * local branch tracking it, or a new one that does), unless that's checked out in a worktree
 * already (here, say): then a new branch, which has to be named, is made from it and checked out
 * there instead; one can be named either way. The worktrees' section in the sidebar shows it
 * running, and why it failed; the folder and name are kept to fix.
 */
export function AddWorktreeDialog(props: {
  repositoryId: string;
  branch: string | undefined;
  onClose: () => void;
}) {
  const repository = useRepository(() => props.repositoryId);
  const worktrees = useWorktrees(() => props.repositoryId);
  const refs = useRefs(() => props.repositoryId);
  const { add } = useWorktreeActions(() => props.repositoryId);
  const [folder, setFolder] = createSignal("");
  const [newBranch, setNewBranch] = createSignal("");
  // The last one while it closes, rather than none.
  const branch = createMemo<string | undefined>((last) => props.branch ?? last);
  const name = () => branchName(branch() ?? "");
  /**
   * Where the branch (for a remote one, the local branch tracking it) is checked out already, if
   * anywhere: then a new branch has to be made.
   */
  const clash = () => checkedOutIn(worktrees.data, refs.data, branch() ?? "");
  const needsNewBranch = () => clash() !== undefined;
  const summary = () => {
    const where = clash();
    if (where) {
      const here = where.current ? "here" : `in ${where.name}`;
      // For a remote branch, it's the local branch tracking it that is.
      const local = where.branch && branchName(where.branch);
      const which = local && local !== name() ? `${local}, which tracks ${name()},` : name();
      return `${which} is checked out ${here}, so a new branch is made from it and checked out in a folder of its own.`;
    }
    const what = branch()?.startsWith("refs/remotes/")
      ? `the local branch tracking ${name()}, or a new one that does,`
      : name();
    return `Checks out ${what} in a folder of its own. Name a new branch to make one from it there instead.`;
  };

  // A folder and a name typed in one repository aren't for another.
  createEffect(
    on(
      () => props.repositoryId,
      () => {
        setFolder("");
        setNewBranch("");
      },
      { defer: true },
    ),
  );
  // The folder last suggested, which a folder that was typed instead is kept over.
  let suggested = "";
  // Suggested afresh for each branch, unless one was typed and kept after a failure.
  createEffect(
    on(
      () => props.branch,
      (open) => {
        if (!open) return;
        // Out of the way of the folder and name, which are kept after a failure to fix them.
        add.dismiss();
        if ((!folder() || folder() === suggested) && repository()) {
          suggested = suggestWorktreeFolder(repository()!.path, branchName(open));
          setFolder(suggested);
        }
      },
    ),
  );

  const valid = () => !!folder().trim() && (!needsNewBranch() || !!newBranch().trim());

  async function browse() {
    const picked = await rpc.system.selectFolder();
    if (picked) setFolder(picked);
  }

  function submit(event: Event) {
    event.preventDefault();
    if (!valid() || add.isPending()) return;
    const ref = branch()!;
    const path = folder().trim();
    const made = newBranch().trim();
    props.onClose();
    // Only what was used: something else may be typed in another repository by then.
    add.run(
      { path, branch: ref, ...(made ? { newBranch: made } : {}) },
      {
        onSuccess: () => {
          setFolder((typed) => (typed.trim() === path ? "" : typed));
          setNewBranch((typed) => (typed.trim() === made ? "" : typed));
        },
      },
    );
  }

  return (
    <Dialog open={!!props.branch} onOpenChange={(open) => !open && props.onClose()} modal>
      <DialogPortal>
        <Dialog.Content class={cn(DIALOG_BOX, "max-w-[400px] p-4")}>
          <form onSubmit={submit}>
            <Dialog.Title class="m-0 text-[12.5px] font-[680]">New worktree</Dialog.Title>
            <Dialog.Description class="m-0 mt-1 mb-2.5 text-[11.5px] leading-[1.45] text-muted">
              {summary()}
            </Dialog.Description>
            <FormField
              label="Folder"
              hint="Empty, or not there yet"
              placeholder="/path/to/folder"
              value={folder()}
              onChange={setFolder}
            />
            <FormField
              label={needsNewBranch() ? "New branch" : "New branch (optional)"}
              placeholder={needsNewBranch() ? "feature/my-change" : `Keep ${name()}`}
              value={newBranch()}
              onChange={setNewBranch}
            />
            <div class="flex gap-2">
              <Button
                type="button"
                variant="secondary"
                class="h-8 shrink-0 rounded-[7px] text-[12.5px] shadow-none"
                onClick={() => void browse()}
              >
                Browse…
              </Button>
              <Button
                type="submit"
                variant="primary"
                icon={FolderPlus}
                disabled={!valid() || add.isPending()}
                class="h-8 flex-1 gap-[7px] rounded-[7px] text-[12.5px] font-[680] shadow-none"
              >
                <Show when={newBranch().trim()} fallback="Add worktree">
                  Add worktree with new branch
                </Show>
              </Button>
            </div>
          </form>
        </Dialog.Content>
      </DialogPortal>
    </Dialog>
  );
}
