// What the release workflow (.github/workflows/release.yml) and CI ask of the changelog. It needs
// nothing installed but Node and git, as Node runs it as it is: `node packages/release/src/cli.ts`.
//
//   previous <version>        The tag the version's changes are counted from, if any.
//   bump [<from>]             What a release of the commits since <from> bumps: major, minor or patch.
//   check-title <title>       Fails unless it's a Conventional Commits title, as a PR's has to be.
//   generate --version <v> [--from <tag>] [--previous <changelog.json>] --out <changelog.json>
//            --notes <notes.md>
//                             Adds the version's changes, since <from>, to the last build's changelog,
//                             and writes the version's release notes.
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { parseArgs } from "node:util";

import {
  addVersion,
  bumpFor,
  type Change,
  type Changelog,
  LOG_FORMAT,
  parseLog,
  parseTitle,
  previousTag,
  releaseNotes,
} from "./changelog.ts";
import { channelOf, SEMVER } from "./version.ts";

/** About three months of daily nightlies; releases are all kept. */
const NIGHTLIES_KEPT = 100;

const [command, ...args] = process.argv.slice(2);

switch (command) {
  case "previous": {
    const [version] = args;
    if (!version || !SEMVER.test(version)) fail(`previous: invalid version ${version}`);
    const tag = previousTag(version, git("tag", "--list").split("\n").filter(Boolean));
    if (tag) console.log(tag);
    break;
  }
  case "bump": {
    console.log(bumpFor(changesSince(args[0])));
    break;
  }
  case "check-title": {
    const parsed = parseTitle(args.join(" "));
    if (!parsed.ok) fail(parsed.reason);
    break;
  }
  case "generate": {
    const { values } = parseArgs({
      args,
      options: {
        version: { type: "string" },
        from: { type: "string" },
        previous: { type: "string" },
        out: { type: "string" },
        notes: { type: "string" },
      },
    });
    const { version, from, previous, out, notes } = values;
    if (!version || !SEMVER.test(version)) fail(`generate: invalid --version ${version}`);
    if (!out || !notes) fail("generate: --out and --notes are needed");
    const changes = changesSince(from);
    const last: Changelog = previous
      ? (JSON.parse(readFileSync(previous, "utf8")) as Changelog)
      : { versions: [] };
    const changelog = addVersion(
      last,
      { version, date: new Date().toISOString().slice(0, 10), changes },
      channelOf(version) === "nightly" ? NIGHTLIES_KEPT : Infinity,
    );
    writeFileSync(out, `${JSON.stringify(changelog, null, 2)}\n`);
    writeFileSync(notes, `${releaseNotes(changes)}\n`);
    console.log(`${version}: ${changes.length} changes since ${from || "the first commit"}`);
    break;
  }
  default:
    fail(`Unknown command ${command}; see packages/release/src/cli.ts.`);
}

/** The changes of the commits since `from`, the newest first; all of them without it. */
function changesSince(from: string | undefined): Change[] {
  return parseLog(
    git("log", "--no-merges", `--format=${LOG_FORMAT}`, from ? `${from}..HEAD` : "HEAD"),
  );
}

function git(...gitArgs: string[]) {
  return execFileSync("git", gitArgs, { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
}

function fail(message: string): never {
  console.error(message);
  process.exit(1);
}
