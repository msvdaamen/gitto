import { relative, resolve } from "node:path";

/**
 * Maps the path of an `app://` URL to a file in the renderer build, or `null` when it points
 * outside of it.
 */
export function resolveRendererPath(rendererDir: string, pathname: string): string | null {
  const filePath = resolve(rendererDir, `.${decodeURIComponent(pathname)}`);
  return relative(rendererDir, filePath).startsWith("..") ? null : filePath;
}
