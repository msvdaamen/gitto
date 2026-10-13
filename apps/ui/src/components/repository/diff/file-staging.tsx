import { patchVersion } from "@gitto/git/patch-version";
import type { ChangedFile } from "@gitto/git/types";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import { createSignal, Show } from "solid-js";

import { Notice } from "@/components/ui/notice";
import { isNestedRepository } from "@/git/changes";
import { isUncommitted } from "@/git/diff-source";
import { hasHunks, stagedWhole } from "@/git/patch";
import { useStageFile, useStageLines } from "@/git/queries/staging";
import { errorMessage } from "@/lib/errors";

import type { LineStaging } from "./patch-viewer";
import type { ShownFile } from "./shown-file";

/**
 * Staging and unstaging an uncommitted file from its diff, `view`: whole, or its lines a hunk or
 * a selection at a time (see `PatchViewer`); one at a time. Once that leaves it without changes
 * on its side, the next file in its list opens.
 */
export function useFileStaging(
  view: ShownFile,
  props: {
    repositoryId: () => string;
    /** Saves the file's edits and stops editing it; resolves to whether it did (`useFileEditing`). */
    leave: () => Promise<boolean>;
    /** Opens another file of the same source's: the one to move on to. */
    open: (file: ChangedFile) => void;
  },
) {
  /** What's done with the file's changes: staged, or unstaged, by the side they're on. */
  const stagingAction = () => (view.source().kind === "staged" ? "unstage" : "stage");
  const stageLines = useStageLines();
  const stageFile = useStageFile();
  /** The whole file is being staged, from saving its edits on. */
  const [stagingWhole, setStagingWhole] = createSignal(false);
  /** Lines, or the whole file, are being staged: nothing else is until they are. */
  const staging = () => stageLines.isPending || stagingWhole();
  /** The patch lines were last staged from: none are again while it's on show and another's in. */
  const [stagedFrom, setStagedFrom] = createSignal<string>();
  /** Why the last lines, or the whole file, couldn't be staged, and of which file. */
  const [stagingError, setStagingError] = createSignal<{ key: string; message: string }>();
  const failed = (key: string, error: unknown) =>
    setStagingError({ key, message: errorMessage(error) });
  /**
   * Once the file `key` names has no changes left on its side, on to `target`. Not if another
   * file was opened meanwhile.
   */
  const moveOn = (key: string, target: ChangedFile | undefined) => {
    if (target && key === view.fileKey()) props.open(target);
  };

  const lineStaging = (): LineStaging | undefined => {
    const data = view.current();
    if (!isUncommitted(view.source()) || view.editing() || !view.showsPatch() || !data) {
      return undefined;
    }
    if (stagedWhole(data.patch)) return undefined;
    const action = stagingAction();
    const last = stagedFrom();
    return {
      action,
      busy:
        staging() ||
        (last !== undefined && view.shownPatch()?.patch === last && data.patch !== last),
      onStage: async (from, picked) => {
        const key = view.fileKey();
        setStagingError(undefined);
        let left: string;
        try {
          left = await stageLines.mutateAsync({
            repositoryId: props.repositoryId(),
            action,
            file: view.file(),
            version: patchVersion(from),
            lines: picked,
          });
        } catch (error) {
          failed(key, error);
          throw error;
        }
        // The patch staged from stays on show until the one they leave is highlighted; once there
        // are none left, so does this file's until the next one's is in.
        setStagedFrom(from);
        if (!hasHunks(left)) moveOn(key, view.target());
      },
    };
  };

  /**
   * Whether the file can be staged, or unstaged, whole: one of the uncommitted changes in its list,
   * but not a repository inside this one, which would be added as a submodule without its URL.
   */
  const wholeFile = () =>
    isUncommitted(view.source()) && view.listed() && !isNestedRepository(view.file())
      ? stagingAction()
      : undefined;
  /**
   * Stages, or unstages, the whole file, its edits saved first so they're staged with it; then on
   * to the next file, as once its last lines are.
   */
  const stageWhole = async () => {
    const action = wholeFile();
    if (!action || staging() || view.showsOther()) return;
    const key = view.fileKey();
    setStagingWhole(true);
    try {
      if (!(await props.leave()) || key !== view.fileKey()) return;
      // Picked while the file's still in its list: it's staged before the list is refetched.
      const target = view.target();
      setStagingError(undefined);
      try {
        await stageFile.mutateAsync({
          repositoryId: props.repositoryId(),
          action,
          file: view.liveFile(),
        });
      } catch (error) {
        failed(key, error);
        return;
      }
      moveOn(key, target);
    } finally {
      setStagingWhole(false);
    }
  };

  return {
    staging,
    /** What the viewer takes to stage lines, if the file's can be. */
    lineStaging,
    wholeFile,
    stageWhole,
    /** Why the last lines, or the whole file, couldn't be staged, while that file's on show. */
    Banner() {
      return (
        <Show when={stagingError()?.key === view.fileKey() && stagingError()}>
          {(error) => <Notice role="alert" icon={TriangleAlert} message={error().message} />}
        </Show>
      );
    },
  };
}
