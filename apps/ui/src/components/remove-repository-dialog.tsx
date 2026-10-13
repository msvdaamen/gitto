import X from "lucide-solid/icons/x";

import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { useLingering } from "@/hooks/lingering";
import type { Repository } from "@/hooks/repositories";

/**
 * Asks before removing the repository `repository` from Gitto, which leaves its folder as it is;
 * open while there's one. Esc, or clicking outside, cancels.
 */
export function RemoveRepositoryDialog(props: {
  repository: Repository | undefined;
  onCancel: () => void;
  onRemove: (repository: Repository) => void;
}) {
  const repository = useLingering(() => props.repository);
  return (
    <ConfirmDialog
      open={props.repository !== undefined}
      icon={X}
      tone="amber"
      title={`Remove ${repository()?.name}?`}
      description={
        <>
          Its tab is closed and it's taken off Gitto's list. The folder {repository()?.path} is kept
          as it is.
        </>
      }
      onCancel={props.onCancel}
    >
      <Button variant="ghost" onClick={props.onCancel}>
        Cancel
      </Button>
      <Button
        variant="primary"
        onClick={() => props.repository && props.onRemove(props.repository)}
      >
        Remove
      </Button>
    </ConfirmDialog>
  );
}
