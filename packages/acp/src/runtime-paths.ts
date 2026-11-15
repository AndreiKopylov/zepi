import { existsSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

// Under a bun-compiled agent, import.meta.url and process.execPath point into the
// exe, so on-disk helpers and the Node runtime are resolved from ZEPI_ROOT instead.
const compiledBun = /~BUN|\$bunfs|%7EBUN/i.test(import.meta.url);

export function acpRoot(): string {
  const root = process.env.ZEPI_ROOT;
  return root ? join(root, "packages", "acp") : fileURLToPath(new URL("..", import.meta.url));
}

// Runtime for our on-disk helpers (preview/terminal). Prefer a bundled node, else
// bun under the compiled agent so standalone does not depend on Node.
export function nodeExe(): string {
  const root = process.env.ZEPI_ROOT;
  if (root) {
    const bundled = join(root, "deps", "node", "node.exe");
    if (existsSync(bundled)) return bundled;
  }
  if (compiledBun) return "bun";
  return process.versions?.bun ? "node" : process.execPath;
}