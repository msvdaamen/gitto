import { AlertDialog } from "@kobalte/core/alert-dialog";
import { useQueryClient } from "@tanstack/solid-query";
import { cn } from "cn";
import ExternalLink from "lucide-solid/icons/external-link";
import RefreshCw from "lucide-solid/icons/refresh-cw";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import { createEffect, createSignal, on, Show } from "solid-js";

import { Button } from "@/components/ui/button";
import { DIALOG_BOX, DialogPortal } from "@/components/ui/dialog";
import { GIT_VERSION_KEY, useGitVersion } from "@/git/queries/version";

export const GIT_DOWNLOAD_URL = "https://git-scm.com/downloads";

/**
 * Says, until it's fixed, that Gitto can't work with the installed git: it's too old, or isn't
 * installed at all. It can't be dismissed, as nothing works meanwhile (see `withRepo`); checking
 * again closes it once git has been updated, and loads again what failed for want of it.
 */
export function GitVersionDialog() {
  const queryClient = useQueryClient();
  const install = useGitVersion();
  // Whether a check the user asked for found the same git as before.
  const [unchanged, setUnchanged] = createSignal(false);

  createEffect(
    on(
      () => install.data?.supported,
      (supported, wasSupported) => {
        if (supported && wasSupported === false) {
          void queryClient.invalidateQueries({
            predicate: (query) => query.queryKey[0] !== GIT_VERSION_KEY[0],
          });
        }
      },
      { defer: true },
    ),
  );

  const checkAgain = async () => {
    const before = install.data?.version;
    setUnchanged(false);
    const { data } = await install.refetch();
    setUnchanged(data?.supported === false && data.version === before);
  };

  return (
    <AlertDialog open={install.data?.supported === false} modal preventScroll>
      <DialogPortal>
        <AlertDialog.Content
          // Nothing works until git is updated, so there's nothing to go back to.
          onEscapeKeyDown={(event) => event.preventDefault()}
          onPointerDownOutside={(event) => event.preventDefault()}
          class={cn(DIALOG_BOX, "max-w-[440px] p-5")}
        >
          <div class="flex items-start gap-3">
            <span class="grid size-9 shrink-0 place-items-center rounded-[9px] bg-coral-soft text-coral">
              <TriangleAlert size={18} strokeWidth={1.9} />
            </span>
            <div class="min-w-0">
              <AlertDialog.Title class="m-0 text-[15px] font-[680]">
                {install.data?.version === null
                  ? "Gitto couldn't find Git"
                  : "Gitto needs a newer version of Git"}
              </AlertDialog.Title>
              <AlertDialog.Description class="m-0 mt-2 flex flex-col gap-2 text-[12.5px] leading-[1.55] text-text-soft">
                <Show
                  when={install.data?.version}
                  fallback={
                    <p class="m-0">
                      Gitto uses Git to read and change your repositories, but Git isn't installed,
                      or isn't on your PATH. Install Git {install.data?.required} or newer to get
                      started.
                    </p>
                  }
                >
                  {(version) => (
                    <p class="m-0">
                      Git <strong class="text-text">{version()}</strong> is installed, but Gitto
                      needs Git <strong class="text-text">{install.data?.required}</strong> or
                      newer. It relies on features older versions of Git don't have, so it can't
                      open your repositories until Git is updated.
                    </p>
                  )}
                </Show>
                <p class="m-0">
                  Once you've installed it, choose Check again. There's no need to restart Gitto.
                </p>
              </AlertDialog.Description>
              <Show when={unchanged()}>
                <p role="status" class="m-0 mt-2 text-[12px] text-coral">
                  {install.data?.version === null
                    ? "Still no Git found. If you've just installed it, make sure it's on your PATH."
                    : `Still finding Git ${install.data?.version}. If you've installed a newer one, make sure it's the first git on your PATH.`}
                </p>
              </Show>
            </div>
          </div>

          <div class="mt-5 flex justify-end gap-2">
            <Button
              icon={RefreshCw}
              disabled={install.isFetching}
              onClick={() => void checkAgain()}
            >
              {install.isFetching ? "Checking…" : "Check again"}
            </Button>
            <Button
              variant="primary"
              icon={ExternalLink}
              onClick={() => window.open(GIT_DOWNLOAD_URL, "_blank")}
            >
              Download Git
            </Button>
          </div>
        </AlertDialog.Content>
      </DialogPortal>
    </AlertDialog>
  );
}
