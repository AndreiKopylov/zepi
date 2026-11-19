import { existsSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

export function acpRoot(): string {
  const root = process.env.ZEPI_ROOT;
  return root ? join(root, "packages", "acp") : fileURLToPath(new URL("..", import.meta.url));
}

// A compiled bun exe always runs its embedded entrypoint and cannot execute external
// scripts; helper .mjs need a real runtime (the exe when uncompiled, else a bun binary).
export function scriptRuntime(): string {
  if (!/~BUN|\$bunfs|%7EBUN/i.test(import.meta.url)) return process.execPath;
  const beside = join(process.execPath, "..", "bun.exe");
  return existsSync(beside) ? beside : "bun";
}