import { Dialog } from "@kobalte/core/dialog";
import { useQuery } from "@tanstack/solid-query";
import { cn } from "cn";
import PenLine from "lucide-solid/icons/pen-line";
import { createEffect, createMemo, createSignal, on, Show } from "solid-js";

import { Button } from "@/components/ui/button";
import { DIALOG_BOX, DialogPortal } from "@/components/ui/dialog";
import { FormField } from "@/components/ui/form-field";
import { editMessage, emptyMessage, splitMessage, type CommitMessage } from "@/git/message";
import { commitMessageQuery, pushedToQuery, useRewordCommit } from "@/git/queries/commit";
import { useLingering } from "@/hooks/lingering";

/**
 * Edits the message of the last commit of the checked-out branch, `sha`, and rewrites the commit
 * with it; open while there's a `sha`. What's staged is left out of the commit, and stays staged.
 * Like amending in the commit form, it says first when the commit is pushed already, as the branch
 * then needs force-pushing. It stays open if that fails, saying why, with the edits kept.
 */
export function RewordDialog(props: {
  repositoryId: string;
  sha: string | undefined;
  onClose: () => void;
  /** Told the commit was reworded, by its SHA before and after. */
  onReworded: (from: string, to: string) => void;
}) {
  const sha = useLingering(() => props.sha);
  const reword = useRewordCommit(() => props.repositoryId);
  const original = useQuery(() => ({
    ...commitMessageQuery(props.repositoryId, sha() ?? ""),
    enabled: !!props.sha,
  }));
  const pushedTo = useQuery(() => ({
    ...pushedToQuery(props.repositoryId, sha() ?? ""),
    enabled: !!props.sha,
  }));
  // Read only once loaded: reading `data` while it loads would suspend the history.
  const loaded = createMemo(() => (original.isSuccess ? splitMessage(original.data) : undefined));
  const [edits, setEdits] = createSignal<CommitMessage>();
  // Each time it opens, it starts from the commit's message, with no failure from before.
  createEffect(
    on(
      () => props.sha,
      (open) => {
        if (!open) return;
        setEdits(undefined);
        reword.reset();
      },
    ),
  );

  const message = () => edits() ?? loaded() ?? emptyMessage;
  const edit = (change: Partial<CommitMessage>) => setEdits({ ...message(), ...change });
  // Also locked if the message failed to load (the error says so), so no edits stand in for it.
  const locked = () => !loaded() || reword.isPending;

  // What wasn't edited of the message, exactly as written, not as the fields split it.
  const edited = () => (original.isSuccess ? editMessage(original.data, message()) : undefined);
  const canSave = () =>
    !!message().summary.trim() &&
    edited() !== undefined &&
    edited() !== original.data &&
    // Once the push check is done too, so its warning is seen before rewording.
    !pushedTo.isPending &&
    !reword.isPending;

  function submit() {
    const from = props.sha;
    const text = edited();
    if (!canSave() || !from || text === undefined) return;
    reword.mutate(
      { sha: from, message: text },
      {
        onSuccess: (to) => {
          props.onReworded(from, to);
          // Unless it's open for another commit by now.
          if (props.sha === from) props.onClose();
        },
      },
    );
  }

  return (
    <Dialog open={!!props.sha} onOpenChange={(open) => !open && props.onClose()} modal>
      <DialogPortal>
        <Dialog.Content class={cn(DIALOG_BOX, "max-w-[440px] p-4")}>
          <form
            onSubmit={(event) => {
              event.preventDefault();
              submit();
            }}
            onKeyDown={(event) => {
              if (event.key !== "Enter" || !(event.metaKey || event.ctrlKey)) return;
              // Else Enter in the summary would submit the form a second time.
              event.preventDefault();
              submit();
            }}
          >
            <Dialog.Title class="m-0 text-[12.5px] font-[680]">Edit commit message</Dialog.Title>
            <Dialog.Description class="m-0 mt-1 mb-2.5 text-[11.5px] leading-[1.45] text-muted">
              Rewrites the last commit with the new message. What's staged isn't added to it.
            </Dialog.Description>
            <FormField
              label="Commit message"
              placeholder={original.isPending ? "Loading the message…" : "Summary of your changes"}
              value={message().summary}
              onChange={(summary) => edit({ summary })}
              readOnly={locked()}
            />
            <FormField
              label="Description"
              hint="optional"
              placeholder="Add more context…"
              rows={5}
              value={message().description}
              onChange={(description) => edit({ description })}
              readOnly={locked()}
            />
            <Show when={pushedTo.isSuccess && pushedTo.data}>
              {(branch) => (
                <p class="m-0 mb-2.5 text-[11.5px] leading-[1.45] text-amber">
                  The commit is already on {branch()}; editing its message means force-pushing.
                </p>
              )}
            </Show>
            <Show when={pushedTo.isError}>
              <p class="m-0 mb-2.5 text-[11.5px] leading-[1.45] text-amber">
                Couldn't check whether the commit has been pushed; if it has, editing its message
                means force-pushing.
              </p>
            </Show>
            {/* Not whether it's been pushed: that only decides the warning. */}
            <Show when={reword.error ?? original.error} keyed>
              {(error) => (
                <p class="m-0 mb-2.5 text-[11.5px] whitespace-pre-wrap text-coral">
                  {error.message}
                </p>
              )}
            </Show>
            <div class="mt-1 flex justify-end gap-2">
              <Button variant="ghost" onClick={() => props.onClose()}>
                Cancel
              </Button>
              <Button type="submit" variant="primary" icon={PenLine} disabled={!canSave()}>
                Save message
              </Button>
            </div>
          </form>
        </Dialog.Content>
      </DialogPortal>
    </Dialog>
  );
}
