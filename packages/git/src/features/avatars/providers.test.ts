import { describe, expect, it, vi } from "vitest";

import { AvatarLookup } from "./providers";
import type { Hosting } from "./remote";

const github: Hosting = { provider: "github", owner: "acme", name: "app" };
const gitlab: Hosting = { provider: "gitlab", host: "gitlab.com", project: "acme/app" };
// SHA-256 of "ada@example.com".
const GRAVATAR =
  "https://www.gravatar.com/avatar/b5fc85e55755f9e0d030a10ab4429b6b2944855f9a0d60077fe832becbc41d72?s=80&d=404";

/** A fake `fetch` answering each URL it's given with `routes`' JSON, or a 404. */
function fakeFetch(routes: Record<string, unknown | Response>) {
  return vi.fn(async (url: string | URL | Request) => {
    const body = routes[String(url)];
    if (body instanceof Response) return body;
    return body === undefined ? new Response("", { status: 404 }) : Response.json(body);
  });
}

const ada = { email: "ada@example.com", sha: "aaaa111" };

describe("AvatarLookup", () => {
  it("finds GitHub avatars among the repository's latest commits", async () => {
    const fetch = fakeFetch({
      "https://api.github.com/repos/acme/app/commits?per_page=100": [
        {
          author: { avatar_url: "https://avatars.githubusercontent.com/u/1" },
          commit: { author: { email: "Ada@Example.com" } },
        },
      ],
    });
    const lookup = new AvatarLookup(fetch);

    expect(await lookup.find(github, ada)).toBe("https://avatars.githubusercontent.com/u/1");
    expect(await lookup.find(github, ada)).toBe("https://avatars.githubusercontent.com/u/1");
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it("finds other GitHub authors through one of their commits", async () => {
    const lookup = new AvatarLookup(
      fakeFetch({
        "https://api.github.com/repos/acme/app/commits?per_page=100": [],
        "https://api.github.com/repos/acme/app/commits/aaaa111": {
          author: { avatar_url: "https://avatars.githubusercontent.com/u/2" },
        },
      }),
    );

    expect(await lookup.find(github, ada)).toBe("https://avatars.githubusercontent.com/u/2");
  });

  it("falls back to Gravatar when GitHub doesn't know the author", async () => {
    const lookup = new AvatarLookup(
      fakeFetch({
        "https://api.github.com/repos/acme/app/commits?per_page=100": [],
        "https://api.github.com/repos/acme/app/commits/aaaa111": { author: null },
      }),
    );

    expect(await lookup.find(github, ada)).toBe(GRAVATAR);
  });

  it("stops asking GitHub once the rate limit is used up, and asks again later", async () => {
    const fetch = fakeFetch({
      "https://api.github.com/repos/acme/app/commits?per_page=100": new Response("", {
        status: 403,
        headers: { "x-ratelimit-remaining": "0", "x-ratelimit-reset": "9999999999" },
      }),
    });
    const lookup = new AvatarLookup(fetch);

    expect(await lookup.find(github, ada)).toBe(GRAVATAR);
    expect(await lookup.find(github, { email: "grace@example.com", sha: "bbbb222" })).toMatch(
      /^https:\/\/www\.gravatar\.com\//,
    );
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it("uses the account of a GitHub noreply address without asking GitHub", async () => {
    const fetch = fakeFetch({});
    const lookup = new AvatarLookup(fetch);

    expect(
      await lookup.find(github, { email: "12345+ada@users.noreply.github.com", sha: "aaaa111" }),
    ).toBe("https://avatars.githubusercontent.com/u/12345?s=80&v=4");
    expect(
      await lookup.find(undefined, { email: "ada@users.noreply.github.com", sha: "a1b2" }),
    ).toBe("https://github.com/ada.png?size=80");
    expect(fetch).not.toHaveBeenCalled();
  });

  it("looks GitLab avatars up by email address", async () => {
    const lookup = new AvatarLookup(
      fakeFetch({
        "https://gitlab.com/api/v4/avatar?email=ada%40example.com&size=80": {
          avatar_url: "/uploads/-/system/user/avatar/1/avatar.png",
        },
      }),
    );

    expect(await lookup.find(gitlab, ada)).toBe(
      "https://gitlab.com/uploads/-/system/user/avatar/1/avatar.png",
    );
  });

  it("prefers our Gravatar over GitLab's, which has a generated default image", async () => {
    const lookup = new AvatarLookup(
      fakeFetch({
        "https://gitlab.com/api/v4/avatar?email=ada%40example.com&size=80": {
          avatar_url: "https://secure.gravatar.com/avatar/abc?s=80&d=identicon",
        },
      }),
    );

    expect(await lookup.find(gitlab, ada)).toBe(GRAVATAR);
  });

  it("uses Gravatar for other hosts, and nothing without an email address", async () => {
    const fetch = fakeFetch({});
    const lookup = new AvatarLookup(fetch);

    expect(await lookup.find(undefined, ada)).toBe(GRAVATAR);
    expect(await lookup.find(github, { email: "", sha: "aaaa111" })).toBeNull();
    expect(fetch).not.toHaveBeenCalled();
  });

  it("asks again after a network error", async () => {
    const fetch = vi.fn<typeof globalThis.fetch>().mockRejectedValue(new TypeError("offline"));
    const lookup = new AvatarLookup(fetch);

    expect(await lookup.find(gitlab, ada)).toBe(GRAVATAR);
    expect(await lookup.find(gitlab, ada)).toBe(GRAVATAR);
    expect(fetch).toHaveBeenCalledTimes(2);
  });
});
