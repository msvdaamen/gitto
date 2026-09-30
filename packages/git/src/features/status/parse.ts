import type { FileStatus, Status, StatusFile } from "./schema";

// `git status --porcelain=v2 -z --branch`; see "Porcelain Format Version 2" in git-status(1).
export const STATUS_ARGS = ["status", "--porcelain=v2", "-z", "--branch", "--untracked-files=all"];

const CODES: Record<string, FileStatus> = {
  M: "modified",
  A: "added",
  D: "deleted",
  R: "renamed",
  C: "copied",
  T: "typechange",
};

export function parseStatus(output: string): Status {
  const status: Status = {
    branch: null,
    head: null,
    upstream: null,
    ahead: 0,
    behind: 0,
    files: [],
  };

  const records = output.split("\0");
  for (let i = 0; i < records.length; i++) {
    const record = records[i]!;
    if (!record) continue;

    switch (record[0]) {
      case "#":
        parseHeader(record, status);
        break;
      // Ordinary change: 1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>
      case "1": {
        const fields = splitFields(record, 9);
        status.files.push(file(fields[8]!, null, fields[1]!));
        break;
      }
      // Rename or copy: 2 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <X><score> <path>, then <origPath>
      case "2": {
        const fields = splitFields(record, 10);
        const origPath = records[++i] ?? null;
        status.files.push(file(fields[9]!, origPath, fields[1]!));
        break;
      }
      // Unmerged: u <XY> <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>
      case "u": {
        const fields = splitFields(record, 11);
        status.files.push({
          path: fields[10]!,
          origPath: null,
          staged: "conflicted",
          unstaged: "conflicted",
        });
        break;
      }
      case "?":
        status.files.push({
          path: record.slice(2),
          origPath: null,
          staged: null,
          unstaged: "untracked",
        });
        break;
      // "!" (ignored) only shows up with --ignored.
    }
  }

  return status;
}

function parseHeader(record: string, status: Status) {
  const [, key, ...rest] = record.split(" ");
  const value = rest.join(" ");
  switch (key) {
    case "branch.oid":
      status.head = value === "(initial)" ? null : value;
      break;
    case "branch.head":
      status.branch = value === "(detached)" ? null : value;
      break;
    case "branch.upstream":
      status.upstream = value;
      break;
    case "branch.ab": {
      const match = /^\+(\d+) -(\d+)$/.exec(value);
      if (match) {
        status.ahead = Number(match[1]);
        status.behind = Number(match[2]);
      }
      break;
    }
  }
}

function file(path: string, origPath: string | null, xy: string): StatusFile {
  return {
    path,
    origPath,
    staged: CODES[xy[0]!] ?? null,
    unstaged: CODES[xy[1]!] ?? null,
  };
}

/** Splits on spaces into at most `count` fields; the last one keeps the rest (paths may contain spaces). */
function splitFields(record: string, count: number): string[] {
  const fields: string[] = [];
  let start = 0;
  for (let n = 1; n < count; n++) {
    const end = record.indexOf(" ", start);
    if (end === -1) break;
    fields.push(record.slice(start, end));
    start = end + 1;
  }
  fields.push(record.slice(start));
  return fields;
}
