import { readFileSync } from "node:fs";
import { readFile, writeFile } from "node:fs/promises";

import { type Changelog, userFacingChanges } from "@gitto/release/changelog";
import { SEMVER } from "@gitto/release/version";
import type { WhatsNew } from "@gitto/system/server";

const NONE: Changelog = { versions: [] };

/**
 * The changelog Gitto was built with: the release workflow puts it beside package.json, and it's
 * packaged as an extra resource (see forge.config.ts). A build of one's own has none.
 */
export function readChangelog(path: string): Changelog {
  try {
    const changelog = JSON.parse(readFileSync(path, "utf8")) as Partial<Changelog>;
    if (!Array.isArray(changelog.versions)) throw new Error("no versions");
    return changelog as Changelog;
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "ENOENT") {
      console.error("[changelog] Couldn't read", path, error);
    }
    return NONE;
  }
}

/**
 * What changed for users since the last version they saw what changed in, which is kept in
 * `lastSeenFile`. The first time Gitto runs, there's nothing to show: it's all new.
 */
export class Changes implements WhatsNew {
  constructor(
    private readonly version: string,
    private readonly changelog: Changelog,
    private readonly lastSeenFile: string,
  ) {}

  async unseen() {
    // A build of one's own is neither in a changelog nor worth remembering.
    if (this.version === "0.0.0") return [];
    const lastSeen = await this.lastSeen();
    const unseen = lastSeen
      ? userFacingChanges(this.changelog, { after: lastSeen, upTo: this.version })
      : [];
    // Nothing to show, or a version older than the last seen: this one counts as seen, so going
    // back to the newer one doesn't show what's been seen already.
    if (!unseen.length && lastSeen !== this.version) await this.seen();
    return unseen;
  }

  all() {
    return userFacingChanges(this.changelog, { upTo: this.version });
  }

  async seen() {
    if (this.version === "0.0.0") return;
    await writeFile(this.lastSeenFile, this.version);
  }

  private async lastSeen() {
    try {
      const version = (await readFile(this.lastSeenFile, "utf8")).trim();
      return SEMVER.test(version) ? version : null;
    } catch {
      return null;
    }
  }
}
