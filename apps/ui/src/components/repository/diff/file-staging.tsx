import type { ChangedFile } from "@gitto/git/types";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import { createSignal, Show } from "solid-js";

import { Notice } from "@/components/ui/notice";
import { isNestedRepository } from "@/git/changes";
import { isUncommitted, type DiffSource } from "@/git/diff-source";
import { hasHunks, stagedWhole } from "@/git/patch";
import type { FilePatch } from "@/git/queries/file-diff";
import { useStageFile, useStageLines } from "@/git/queries/staging";
import { errorMessage } from "@/lib/errors";

import type { LineStaging } from "./patch-viewer";

/**
 * Staging and unstaging an uncommitted file from its diff: whole, or its lines a hunk or a
 * selection at a time (see `PatchViewer`); one at a time. Once that leaves it without changes on
 * its side, the next file in its list opens.
 */
export function useFileStaging(props: {
  repositoryId: () => string;
  /** Where the file's changes are from; its side is the one they're staged from, or unstaged to. */
  source: () => DiffSource;
  /** Names the file on show in its source (see `diffFileKey`). */
  fileKey: () => string;
  /** The file, as it's kept while it's edited. */
  file: () => ChangedFile;
  /** The file as its list has it now. */
  liveFile: () => ChangedFile;
  /** Whether the file is still in its list. */
  listed: () => boolean;
  /** The patch of this file, if it's loaded. */
  current: () => FilePatch | undefined;
  /** The patch on show, if one is: the last file's, while this one's loads. */
  shownPatch: () => string | undefined;
  /** Whether the file in the header is another one, still on show while this one's changes load. */
  showsOther: () => boolean;
  editing: () => boolean;
  /** Whether the file's changes are shown as a patch, rather than said to be binary, say. */
  showsPatch: () => boolean;
  /**
   * The file to open once this one has no changes left on its side: the next in its list, or the
   * one before the last, as one would stage them one after the other.
   */
  target: () => ChangedFile | undefined;
  /** Saves the file's edits and stops editing it; resolves to whether it did (`useFileEditing`). */
  leave: () => Promise<boolean>;
  /** Opens another file of the same source's: the one to move on to. */
  open: (file: ChangedFile) => void;
}) {
  /** What's done with the file's changes: staged, or unstaged, by the side they're on. */
  const stagingAction = () => (props.source().kind === "staged" ? "unstage" : "stage");
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
    if (target && key === props.fileKey()) props.open(target);
  };

  const lineStaging = (): LineStaging | undefined => {
    const data = props.current();
    if (!isUncommitted(props.source()) || props.editing() || !props.showsPatch() || !data) {
      return undefined;
    }
    if (stagedWhole(data.patch)) return undefined;
    const action = stagingAction();
    const last = stagedFrom();
    return {
      action,
      busy: staging() || (last !== undefined && props.shownPatch() === last && data.patch !== last),
      onStage: async (from, picked) => {
        const key = props.fileKey();
        setStagingError(undefined);
        let left: string;
        try {
          left = await stageLines.mutateAsync({
            repositoryId: props.repositoryId(),
            action,
            file: props.file(),
            patch: from,
            lines: picked,
          });
        } catch (error) {
          failed(key, error);
          throw error;
        }
        // The patch staged from stays on show until the one they leave is highlighted; once there
        // are none left, so does this file's until the next one's is in.
        setStagedFrom(from);
        if (!hasHunks(left)) moveOn(key, props.target());
      },
    };
  };

  /**
   * Whether the file can be staged, or unstaged, whole: one of the uncommitted changes in its list,
   * but not a repository inside this one, which would be added as a submodule without its URL.
   */
  const wholeFile = () =>
    isUncommitted(props.source()) && props.listed() && !isNestedRepository(props.file())
      ? stagingAction()
      : undefined;
  /**
   * Stages, or unstages, the whole file, its edits saved first so they're staged with it; then on
   * to the next file, as once its last lines are.
   */
  const stageWhole = async () => {
    const action = wholeFile();
    if (!action || staging() || props.showsOther()) return;
    const key = props.fileKey();
    setStagingWhole(true);
    try {
      if (!(await props.leave()) || key !== props.fileKey()) return;
      // Picked while the file's still in its list: it's staged before the list is refetched.
      const target = props.target();
      setStagingError(undefined);
      try {
        await stageFile.mutateAsync({
          repositoryId: props.repositoryId(),
          action,
          file: props.liveFile(),
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
        <Show when={stagingError()?.key === props.fileKey() && stagingError()}>
          {(error) => <Notice role="alert" icon={TriangleAlert} message={error().message} />}
        </Show>
      );
    },
  };
}
