import type { FileStatus } from "../../schema";
import type { ChangedFile } from "./schema";

const CODES: Record<string, FileStatus> = {
  A: "added",
  C: "copied",
  D: "deleted",
  M: "modified",
  R: "renamed",
  T: "typechange",
  U: "conflicted",
};

/**
 * Parses `--raw --numstat -z` output: first a raw entry per file, then a numstat entry per file.
 *
 * - raw: `:<mode> <mode> <sha> <sha> <status>\0<path>\0`, or for renames and copies
 *   `:<mode> <mode> <sha> <sha> R<score>\0<origPath>\0<path>\0`
 * - numstat: `<added>\t<deleted>\t<path>\0`, or for renames and copies
 *   `<added>\t<deleted>\t\0<origPath>\0<path>\0`. Binary files report `-` for both counts.
 */
export function parseDiff(output: string): ChangedFile[] {
  const records = output.split("\0");
  const files = new Map<string, ChangedFile>();

  for (let i = 0; i < records.length; i++) {
    const record = records[i]!;
    if (!record) continue;

    if (record.startsWith(":")) {
      const code = record.slice(record.lastIndexOf(" ") + 1);
      const status = CODES[code[0]!] ?? "modified";
      const origPath = status === "renamed" || status === "copied" ? records[++i]! : null;
      const path = records[++i]!;
      // A conflicted file is listed again as modified, compared to one side of the conflict: it
      // stays conflicted, as the status has it.
      if (files.get(path)?.status !== "conflicted") {
        files.set(path, { path, status, origPath, additions: null, deletions: null });
      }
      continue;
    }

    const firstTab = record.indexOf("\t");
    const secondTab = record.indexOf("\t", firstTab + 1);
    if (firstTab === -1 || secondTab === -1) continue;

    let path = record.slice(secondTab + 1);
    if (!path) {
      i++; // origPath, already known from the raw entry
      path = records[++i] ?? "";
    }
    const file = files.get(path);
    if (!file) continue;

    const additions = record.slice(0, firstTab);
    const deletions = record.slice(firstTab + 1, secondTab);
    file.additions = additions === "-" ? null : Number(additions);
    file.deletions = deletions === "-" ? null : Number(deletions);
  }

  return [...files.values()];
}
