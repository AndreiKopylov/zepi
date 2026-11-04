import { lstat, realpath } from "node:fs/promises";
import { dirname, basename, isAbsolute, join, relative, resolve, sep } from "node:path";

/** Read-only roots outside the project: skills and commands the agent must be able to read. */
export function readRoots() {
  const roots: string[] = [];
  // pi exposes its config directory as PI_CODING_AGENT_DIR; Zepi's own name only exists in the host process.
  const agentDir = process.env.PI_CODING_AGENT_DIR ?? process.env.ZEPI_PI_CONFIG_DIR;
  if (agentDir) roots.push(join(agentDir, "skills"), join(agentDir, "commands"));
  // cli.ts rewrites PI_CODING_AGENT_DIR, so the bundled and per-user skill locations are
  // also taken from paths the agent always has.
  const root = process.env.ZEPI_ROOT;
  if (root) roots.push(join(root, "native", "pi-defaults", "skills"));
  const local = process.env.LOCALAPPDATA;
  if (local) roots.push(join(local, "Zepi", "pi", "skills"), join(local, "Zepi", "pi", "commands"));
  const home = process.env.USERPROFILE ?? process.env.HOME;
  if (home) roots.push(join(home, ".agents", "skills"));
  // Document viewers write slide and page images here and hand the path to the agent.
  if (local) roots.push(join(local, "Zepi", "temp"), join(local, "Zepi", "render"));
  return roots;
}

async function underReadRoot(canonical: string) {
  for (const root of readRoots()) {
    const real = await realpath(root).catch(() => undefined);
    if (real && (canonical === real || canonical.startsWith(real + sep))) return true;
  }
  return false;
}

/** Like workspacePath, but also accepts the read-only skill roots; writes never use it. */
export async function readableFile(cwd: string, path: string) {
  try {
    return { canonical: await workspacePath(cwd, path, false), insideWorkspace: true };
  } catch (error) {
    const canonical = await realpath(resolve(cwd, path)).catch(() => undefined);
    if (canonical && (await underReadRoot(canonical))) return { canonical, insideWorkspace: false };
    // A refused read outside the project is a configuration question; the agent log shows why.
    console.error(`zepi file tools refused ${path}: canonical=${canonical ?? "?"} roots=${readRoots().join(",")}`);
    throw error;
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
