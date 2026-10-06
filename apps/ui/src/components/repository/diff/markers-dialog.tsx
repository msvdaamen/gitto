import TriangleAlert from "lucide-solid/icons/triangle-alert";

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
  return (
    <ConfirmDialog
      item={props.open ? props.path : undefined}
      icon={TriangleAlert}
      tone="amber"
      title={(path) =>
        `Mark ${path.slice(path.lastIndexOf("/") + 1)} resolved with its conflict markers?`
      }
      description={() =>
        `${
          props.left > 0
            ? `It still has ${props.left} ${props.left === 1 ? "conflict" : "conflicts"}.`
            : "It still has conflict markers."
        } Mark it resolved only if they belong in it, like a test's fixture of them: they're staged as they are.`
      }
      confirmLabel="Mark resolved"
      onCancel={props.onCancel}
      onConfirm={() => props.onConfirm()}
    />
  );
}
