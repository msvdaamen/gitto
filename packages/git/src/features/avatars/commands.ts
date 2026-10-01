import type { Repo } from "../../core/repo";
import { AvatarLookup } from "./providers";
import { getHosting } from "./remote";

const lookup = new AvatarLookup();

/**
 * The avatar of the author of commit `sha`, by their email address: from the repository's host if
 * it's on GitHub or GitLab, from Gravatar otherwise. `null` without an email address.
 */
export async function getAvatar(
  repo: Repo,
  author: { email: string; sha: string },
  signal?: AbortSignal,
): Promise<string | null> {
  const hosting = await getHosting(repo, signal);
  return lookup.find(hosting, { email: author.email.trim().toLowerCase(), sha: author.sha });
}
