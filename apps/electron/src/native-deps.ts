import { existsSync } from "node:fs";
import { cp, readFile, realpath } from "node:fs/promises";
import { dirname, join } from "node:path";

// Used when packaging (see forge.config.ts), not by the app itself. Vite bundles everything else,
// but native modules can't be bundled, so they ship as `node_modules` next to the bundle.

interface PackageJson {
  dependencies?: Record<string, string>;
  optionalDependencies?: Record<string, string>;
}

/**
 * The folders of `names` and of everything they depend on, by package name, as installed from
 * `fromDir`. Optional dependencies that aren't installed (e.g. for another platform) are skipped.
 */
export async function findDependencies(
  names: string[],
  fromDir: string,
): Promise<Map<string, string>> {
  const found = new Map<string, string>();

  async function visit(name: string, from: string, optional: boolean) {
    const dir = await findPackage(name, from);
    if (!dir) {
      if (optional) return;
      throw new Error(`Can't find ${name}, needed from ${from}.`);
    }
    const existing = found.get(name);
    if (existing === dir) return;
    // `copyDependencies` puts every package at the top level, so there's room for one version only.
    if (existing) throw new Error(`Two versions of ${name} are needed: ${existing} and ${dir}.`);
    found.set(name, dir);

    const pkg = JSON.parse(await readFile(join(dir, "package.json"), "utf8")) as PackageJson;
    await Promise.all([
      ...Object.keys(pkg.dependencies ?? {}).map((dependency) => visit(dependency, dir, false)),
      ...Object.keys(pkg.optionalDependencies ?? {}).map((dependency) =>
        visit(dependency, dir, true),
      ),
    ]);
  }

  await Promise.all(names.map((name) => visit(name, fromDir, false)));
  return found;
}

/** Copies packages (from `findDependencies`) to `buildPath/node_modules`, all at the top level. */
export async function copyDependencies(packages: Map<string, string>, buildPath: string) {
  await Promise.all(
    [...packages].map(([name, dir]) =>
      cp(dir, join(buildPath, "node_modules", name), { recursive: true, dereference: true }),
    ),
  );
}

/** Where Node would find package `name` from `from`, with symlinks (pnpm's) resolved. */
async function findPackage(name: string, from: string): Promise<string | undefined> {
  for (let dir = from; ; dir = dirname(dir)) {
    const candidate = join(dir, "node_modules", name);
    if (existsSync(join(candidate, "package.json"))) return realpath(candidate);
    if (dirname(dir) === dir) return undefined;
  }
}
