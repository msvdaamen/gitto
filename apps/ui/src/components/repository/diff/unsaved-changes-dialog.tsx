import TriangleAlert from "lucide-solid/icons/triangle-alert";

import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";

/** What to do with edits that couldn't be saved, before going on. */
export type UnsavedChoice = "save" | "discard" | "cancel";

/**
 * Asks what to do with a file's edits that couldn't be saved, before what the user asked for (like
 * closing the file) goes on: save them anyway, drop them, or stay. Esc, or clicking outside, stays.
 */
export function UnsavedChangesDialog(props: {
  open: boolean;
  path: string;
  /** Why they weren't saved. */
  reason: string;
  /** The file changed on disk since: saving overwrites that. */
  changedOnDisk: boolean;
  onChoose: (choice: UnsavedChoice) => void;
}) {
  const name = () => props.path.slice(props.path.lastIndexOf("/") + 1);
  return (
    <ConfirmDialog
      open={props.open}
      icon={TriangleAlert}
      tone="amber"
      title={`Save your changes to ${name()}?`}
      description={
        <>
          {props.reason}{" "}
          {props.changedOnDisk
            ? "Saving replaces the file with your version."
            : "Your latest changes aren't saved yet."}
        </>
      }
      onCancel={() => props.onChoose("cancel")}
    >
      <Button variant="ghost" onClick={() => props.onChoose("cancel")}>
        Cancel
      </Button>
      <Button onClick={() => props.onChoose("discard")}>Discard</Button>
      <Button variant="primary" onClick={() => props.onChoose("save")}>
        {props.changedOnDisk ? "Overwrite" : "Save"}
      </Button>
    </ConfirmDialog>
  );
}
