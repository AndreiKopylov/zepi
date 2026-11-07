import { lstat, realpath } from "node:fs/promises";
import { dirname, basename, isAbsolute, relative, resolve, sep } from "node:path";

/** Reads follow the product full-access rule; writes and private storage stay restricted. */
export async function readableFile(cwd: string, path: string) {
  try {
    return { canonical: await workspacePath(cwd, path, false), insideWorkspace: true };
  } catch (error) {
    if ((error as Error).message.startsWith("Private workspace storage")) throw error;
    const canonical = await realpath(resolve(cwd, path)).catch(() => undefined);
    if (!canonical) throw error;
    return { canonical, insideWorkspace: false };
  }
}

export async function workspacePath(cwd: string, path: string, allowMissing = false) {
  const root = await realpath(cwd);
  let ancestor = resolve(cwd, path);
  const suffix: string[] = [];
  let canonical: string;
  for (;;) {
    try { canonical = resolve(await realpath(ancestor), ...suffix); break; }
    catch (error) {
      if (!allowMissing || (error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
      // A dangling symlink is not a new file. Do not reinterpret its target.
      const entry = await lstat(ancestor).catch((error: NodeJS.ErrnoException) => {
        if (error.code !== "ENOENT") throw error;
        return undefined;
      });
      if (entry || dirname(ancestor) === ancestor) throw error;
      suffix.unshift(basename(ancestor)); ancestor = dirname(ancestor);
    }
  }
  const local = relative(root, canonical);
  if (local === ".." || local.startsWith(`..${sep}`) || isAbsolute(local)) {
    throw new Error("File is outside this workspace.");
  }
  if (local.split(sep).some(part => [".git", ".local", ".pi"].includes(part) || part.startsWith('.zepi-review-'))) {
    throw new Error("Private workspace storage is not available to file tools.");
  }
  return canonical;
}
