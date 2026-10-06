import TriangleAlert from "lucide-solid/icons/triangle-alert";

import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";

/**
 * Asks before marking a file resolved with conflict markers left in it, which is only right when
 * they belong in it, like a test's fixture of them: they're staged as they are. Esc, or clicking
 * outside, cancels.
 */
export function MarkersDialog(props: {
  open: boolean;
  path: string;
  /** How many conflicts the view reads in it; none when its markers can't be read as conflicts. */
  left: number;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const name = () => props.path.slice(props.path.lastIndexOf("/") + 1);
  return (
    <ConfirmDialog
      open={props.open}
      icon={TriangleAlert}
      tone="amber"
      title={`Mark ${name()} resolved with its conflict markers?`}
      description={
        <>
          {props.left > 0
            ? `It still has ${props.left} ${props.left === 1 ? "conflict" : "conflicts"}.`
            : "It still has conflict markers."}{" "}
          Mark it resolved only if they belong in it, like a test's fixture of them: they're staged
          as they are.
        </>
      }
      onCancel={props.onCancel}
    >
      <Button variant="ghost" onClick={props.onCancel}>
        Cancel
      </Button>
      <Button variant="primary" onClick={props.onConfirm}>
        Mark resolved
      </Button>
    </ConfirmDialog>
  );
}
