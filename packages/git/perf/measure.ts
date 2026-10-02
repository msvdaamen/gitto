// Helpers for the perf suites: which repositories to run on, and printing results.
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { basename, join, resolve } from "node:path";

import type { RepositoryService } from "@gitto/repository/server";

import { GitReposImpl, type Repo } from "../src/core/repo";

/** How often each operation runs; the first run is a warm-up and isn't counted. */
export const RUNS = Number(process.env.GITTO_PERF_RUNS ?? 10);

/** The repositories in GITTO_PERF_REPOS (comma-separated), or this one. */
export function perfRepoPaths(): string[] {
  const paths = process.env.GITTO_PERF_REPOS?.split(",").filter(Boolean) ?? [];
  if (paths.length === 0) return [resolve(import.meta.dirname, "../../..")];
  return paths.map((path) => resolve(path.replace(/^~(?=\/|$)/, homedir())));
}

/** Opens `path` the way the app does, without a database. */
export function openRepo(path: string): Promise<Repo> {
  const repos = new GitReposImpl({
    getRepository: async (id: string) => ({ id, name: basename(path), path }),
  } as RepositoryService);
  return repos.open(path);
}

export function git(cwd: string, ...args: string[]): string {
  return execFileSync("git", args, { cwd, encoding: "utf8", maxBuffer: 1 << 30 }).trim();
}

/** A one-line summary of the repository's size, which is what the timings depend on. */
export function describeRepo(path: string): string {
  const commits = git(path, "rev-list", "--count", "--all");
  const files = git(path, "ls-files", "-z").split("\0").length - 1;
  const refs = git(path, "for-each-ref", "--format=x").split("\n").length;
  const objects = join(
    git(path, "rev-parse", "--path-format=absolute", "--git-common-dir"),
    "objects/info",
  );
  const commitGraph =
    existsSync(join(objects, "commit-graph")) || existsSync(join(objects, "commit-graphs"));
  return `${basename(path)}: ${commits} commits, ${files} files, ${refs} refs, ${commitGraph ? "with" : "no"} commit-graph`;
}

export function median(values: number[]): number {
  return percentile(values, 0.5);
}

export function percentile(values: number[], p: number): number {
  const sorted = values.toSorted((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))] ?? 0;
}

/** Milliseconds, with one decimal below 10ms where the difference matters. */
export function ms(value: number): string {
  return value < 10 ? value.toFixed(1) : value.toFixed(0);
}

export function bytes(value: number): string {
  if (value < 1024) return `${value}B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(0)}KB`;
  return `${(value / 1024 / 1024).toFixed(1)}MB`;
}

/** Prints rows as an aligned table: the first column left-aligned, the rest right-aligned. */
export function printTable(title: string, rows: Record<string, string>[]): void {
  const columns = Object.keys(rows[0] ?? {});
  const widths = columns.map((column) =>
    Math.max(column.length, ...rows.map((row) => row[column]?.length ?? 0)),
  );
  const line = (cells: string[]) =>
    cells
      .map((cell, i) => (i === 0 ? cell.padEnd(widths[i]!) : cell.padStart(widths[i]!)))
      .join("  ");
  console.log(
    `\n${title}\n${line(columns)}\n${rows.map((row) => line(columns.map((c) => row[c] ?? ""))).join("\n")}`,
  );
}
