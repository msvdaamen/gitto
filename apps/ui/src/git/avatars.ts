import { useQueryClient } from "@tanstack/solid-query";
import { createEffect, createSignal, onCleanup, type Accessor } from "solid-js";

import { rpc } from "@/lib/rpc";
import type { Commit } from "@/types/git";

import { gitKeys } from "./keys";

/** Whose avatar to show: the author of commit `sha`. */
export interface AvatarAuthor {
  repositoryId: string;
  email: string;
  sha: string;
}

/** The author of a history row whose avatar to show; there's none for the uncommitted changes. */
export function avatarAuthor(commit: Commit): AvatarAuthor | undefined {
  if (commit.isWip || !commit.email) return undefined;
  return { repositoryId: commit.repositoryId, email: commit.email, sha: commit.id };
}

/** Pictures that failed to load (no profile picture, or offline), so they aren't tried again. */
const [failed, setFailed] = createSignal(new Set<string>());

/**
 * The author's profile picture (see `rpc.git.avatars.find`), or `undefined` while it's being
 * looked up, when there is none, or once it failed to load. Call `onError` when the picture fails
 * to load.
 */
export function useAvatar(author: () => AvatarAuthor | undefined): {
  url: Accessor<string | undefined>;
  onError: () => void;
} {
  const queryClient = useQueryClient();
  // Not `useQuery`: reading it would suspend the history table while the avatar is looked up.
  const [resolved, setResolved] = createSignal<string | null>();
  createEffect(() => {
    const value = author();
    if (!value?.email) {
      setResolved(undefined);
      return;
    }
    const queryKey = gitKeys.avatar(value.repositoryId, value.email);
    // Known avatars are set right away, so a row that's reused for another commit doesn't flash
    // its initials in between.
    const known = queryClient.getQueryData<string | null>(queryKey);
    setResolved(known);
    if (known !== undefined) return;
    let current = true;
    onCleanup(() => (current = false));
    queryClient
      .fetchQuery({
        queryKey,
        queryFn: ({ signal }) =>
          rpc.git.avatars.find(
            { repositoryId: value.repositoryId, email: value.email, sha: value.sha },
            { signal },
          ),
        staleTime: Infinity,
      })
      .then(
        (url) => current && setResolved(url),
        () => undefined,
      );
  });
  const url = () => {
    const value = resolved();
    return value && !failed().has(value) ? value : undefined;
  };
  return {
    url,
    onError: () => {
      const value = url();
      if (value) setFailed((set) => new Set(set).add(value));
    },
  };
}
