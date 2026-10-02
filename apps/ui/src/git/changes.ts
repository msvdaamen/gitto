import type { ChangedFile } from "@gitto/git/types";

/** Lines added and deleted across `files`; binary files don't count. */
export function lineTotals(files: ChangedFile[]) {
  return files.reduce(
    (sum, file) => ({
      additions: sum.additions + (file.additions ?? 0),
      deletions: sum.deletions + (file.deletions ?? 0),
    }),
    { additions: 0, deletions: 0 },
  );
}

/** Paths to stage or unstage for files; renames need their old path too, for the deletion side. */
export function stagingPaths(files: ChangedFile[]): string[] {
  return files.flatMap((file) => (file.origPath ? [file.path, file.origPath] : [file.path]));
}
