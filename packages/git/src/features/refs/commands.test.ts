import { describe, expect, it } from "vitest";

import { createHistoryRepo, createRepo, repos } from "../../test/fixtures";
import { listRefs } from "./commands";

describe("listRefs", () => {
  it("lists branches and tags, marking the checked-out branch", async () => {
    const { refs } = await listRefs(await createHistoryRepo());
    expect(refs.map((ref) => `${ref.kind}:${ref.name}${ref.current ? "*" : ""}`)).toEqual([
      "local:main*",
      "local:side",
      "tag:v1",
    ]);
  });

  it("lists nothing in a repository without commits", async () => {
    createRepo("empty");
    expect((await listRefs(await repos.open("empty"))).refs).toEqual([]);
  });
});
